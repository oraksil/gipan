extern crate libemu;

mod encoder;

use std::{
    env,
    mem,
    sync::Arc,
    thread,
    ops::Sub,
    time::{SystemTime, Duration, UNIX_EPOCH},
};

use atoi::atoi;
use nanomsg::{Socket, Protocol};

use crossbeam_channel as channel;

use av_data;
use av_data::pixel::formats::YUV420;
use av_data::pixel::Formaton;
use av_codec;
use av_codec::common::CodecList;

use libvpx::encoder::VP9_DESCR;
use libopus::encoder::OPUS_DESCR;

use libemu::{
    Emulator,
    MameEmulator,
    EmuImageFrame,
    EmuSoundFrame,
    EmuInputEvent,
    InputKind,
};

use encoder::converter;

const FRAME_EXPIRE_DURATION: Duration = Duration::from_millis(30);
const CHANNEL_BUF_SIZE: usize = 64;

#[derive(Debug, Default, Copy, Clone)]
struct Resolution {
    w: usize,
    h: usize,
}

impl Resolution {
    fn from_size(w: usize, h: usize) -> Resolution {
        Resolution { w, h }
    }
}

#[derive(Debug, Default)]
struct GameProperties {
    resolution: Resolution,
    fps: usize,
    keyframe_interval: i32,
    system_name: String,
    imageframe_output: String,
    soundframe_output: String,
    key_input: String,
}

struct EmuEncSoundFrame {
    pub buf: Vec<u8>,
}

fn parse_resolution(arg: String) -> (usize, usize) {
    let whs: Vec<usize> = arg.split("x")
        .map(|s| s.parse().unwrap())
        .collect();

    (whs[0], whs[1])
}

fn extract_properties_from_args(args: &Vec<String>) -> GameProperties {
    let mut props = GameProperties::default();

    // default props
    props.resolution = Resolution::from_size(480, 320);
    props.fps = 30;
    props.keyframe_interval = 12;
    props.imageframe_output = String::from("ipc://./imageframes.ipc");
    props.soundframe_output = String::from("ipc://./soundframes.ipc");

    for (i, arg) in args.iter().map(|s| s.as_str()).enumerate() {
        let next_arg = || { args[i+1].clone() };
        match arg {
            "--game" => {
                props.system_name = next_arg()
            }
            "--imageframe-output" => {
                props.imageframe_output = next_arg()
            },
            "--soundframe-output" => {
                props.soundframe_output = next_arg()
            },
            "--key-input" => {
                props.key_input = next_arg()
            },
            "--fps" => {
                props.fps = next_arg().parse().unwrap()
            },
            "--keyframe-interval" => {
                props.keyframe_interval = next_arg().parse().unwrap()
            },
            "--resolution" => {
                let (w, h) = parse_resolution(next_arg());
                props.resolution = Resolution::from_size(w, h);
            },
            _ => {
                if arg.starts_with("--") {
                    panic!("invalid args have been passed");
                }
            }
        }
    }

    props
}

fn now_utc() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
}

fn run_frame_encoder(
    props: &GameProperties,
    encoder_rx: channel::Receiver<EmuImageFrame>,
    frame_tx: channel::Sender<EmuImageFrame>) {

    let r = props.resolution;
    let fps = props.fps as i64;
    let kf_interval = props.keyframe_interval as i64;

    const YUV_FMT_420: Formaton = *YUV420;
    let codec_info = av_data::params::VideoInfo {
        width: r.w,
        height: r.h,
        format: Some(Arc::new(YUV_FMT_420)),
    };
    let i_frame_info = av_data::frame::VideoInfo {
        pic_type: av_data::frame::PictureType::I,
        width: r.w,
        height: r.h,
        format: Arc::new(YUV_FMT_420),
    };
    let p_frame_info = av_data::frame::VideoInfo {
        pic_type: av_data::frame::PictureType::P,
        width: r.w,
        height: r.h,
        format: Arc::new(YUV_FMT_420),
    };
    let codec_params = av_data::params::CodecParams {
        kind: Some(av_data::params::MediaKind::Video(codec_info)),
        codec_id: Some(String::from("vpx")),
        extradata: None,
        bit_rate: 0,
        convergence_window: 0,
        delay: 0,
    };

    let encoder_list = av_codec::encoder::Codecs::from_list(&[VP9_DESCR]);
    let mut enc_ctx = av_codec::encoder::Context::by_name(&encoder_list, &"vp9").unwrap();

    enc_ctx.set_params(&codec_params).unwrap();
    enc_ctx.configure().unwrap();
    enc_ctx.set_option("cpu-used", 4u64).unwrap();

    thread::spawn(move || {
        let yuv_size = r.w * r.h;
        let chroma_size = yuv_size / 4;

        // Regarding timebase and pts,
        // https://stackoverflow.com/questions/43333542/what-is-video-timescale-timebase-or-timestamp-in-ffmpeg/43337235
        let mut frame_idx = 0;
        let mut encoded_frame_cnt = 0;

        loop {
            let raw_frame = encoder_rx.recv().unwrap();
            // println!("raw frame size: {}", raw_frame.buf.len());

            let now = now_utc();
            let expired = now.sub(FRAME_EXPIRE_DURATION);
            if raw_frame.timestamp < expired {
                println!("raw frame is decayed, dropping.. {:?} < {:?}", raw_frame.timestamp, expired);
                frame_idx += 1;
                continue;
            }

            let mut y = vec![0u8; yuv_size];
            let mut u = vec![0u8; chroma_size];
            let mut v = vec![0u8; chroma_size];
            converter::bgra_to_yuv420(r.w, r.h, &raw_frame.buf, y.as_mut(), u.as_mut(), v.as_mut());
            // println!("yuv frame size: y: {}, u: {}, v: {}", y.len(), u.len(), v.len());

            let yuv_bufs = [y, u, v];
            let source = yuv_bufs.iter().map(|v| v.as_ref());

            // https://stackoverflow.com/questions/13286022/can-anyone-help-in-understanding-avframe-linesize
            let yuv_strides = [r.w, r.w / 2, r.w / 2];
            let linesizes = yuv_strides.iter().map(|v| *v);

            let av_frame = {
                let time_info = {
                    let mut ti: av_data::timeinfo::TimeInfo = av_data::timeinfo::TimeInfo::default();
                    ti.timebase = Some(av_data::rational::Rational64::new(1, fps));
                    ti.pts = Some(frame_idx);
                    ti
                };

                let frame_kind = if encoded_frame_cnt % kf_interval == 0 {
                    av_data::frame::MediaKind::Video(i_frame_info.clone())
                } else {
                    av_data::frame::MediaKind::Video(p_frame_info.clone())
                };
                let arc_frame = {
                    let mut f = av_data::frame::new_default_frame(frame_kind, Some(time_info));
                    f.copy_from_slice(source, linesizes);
                    Arc::new(f)
                };
                arc_frame
            };

            let encoded_frame = {
                enc_ctx.send_frame(&av_frame).unwrap();
                enc_ctx.flush().unwrap();

                let packet = enc_ctx.receive_packet().unwrap();
                let buf = Vec::from(packet.data);
                // println!("encoded frame size: {}", buf.len());

                EmuImageFrame { buf: buf, timestamp: raw_frame.timestamp }
            };

            frame_tx.send(encoded_frame).unwrap();

            frame_idx += 1;
            encoded_frame_cnt += 1;
        }
    });
}

fn run_frame_handler(props: &GameProperties, frame_rx: channel::Receiver<EmuImageFrame>) {
    let frame_output_path = String::from(&props.imageframe_output);
    assert!(frame_output_path.starts_with("ipc://"));

    thread::spawn(move || {
        let mut socket = Socket::new(Protocol::Push).unwrap();
        socket.bind(&frame_output_path).unwrap();

        loop {
            let frame: EmuImageFrame = frame_rx.recv().unwrap();
            match socket.nb_write(frame.buf.as_ref()) {
                Ok(_) => {
                    // println!("sent frame to nanomsg q: {}", sent);
                },
                Err(_) => {
                    // println!("problem while writing: {}", err);
                }
            }
        }
    });
}

fn copy_interleaved_sound_samples(src: &Vec<i16>, dst_frame: &mut av_data::frame::Frame) {
    let samples = src.len() / 2;

    let l = {
        let buf = dst_frame.buf.as_mut_slice_inner(0).unwrap();
        unsafe { mem::transmute::<&mut [u8], &mut [i16]>(buf) }
    };
    let r = {
        let buf = dst_frame.buf.as_mut_slice_inner(1).unwrap();
        unsafe { mem::transmute::<&mut [u8], &mut [i16]>(buf) }
    };
    for i in 0..samples {
        l[i] = src[i*2];
        r[i] = src[i*2+1];
    }
}

fn copy_interleaved_sound_samples_mono(src: &Vec<i16>, dst_frame: &mut av_data::frame::Frame) {
    let samples = src.len() / 2;

    let b = {
        let buf = dst_frame.buf.as_mut_slice_inner(0).unwrap();
        unsafe { mem::transmute::<&mut [u8], &mut [i16]>(buf) }
    };
    for i in 0..samples {
        b[i] = src[i*2];
    }
}

fn run_sound_encoder(
    props: &GameProperties,
    encoder_rx: channel::Receiver<EmuSoundFrame>,
    frame_tx: channel::Sender<EmuEncSoundFrame>) {

    let fps = props.fps as i64;

    let encoder_list = av_codec::encoder::Codecs::from_list(&[OPUS_DESCR]);
    let mut enc_ctx = av_codec::encoder::Context::by_name(&encoder_list, &"opus").unwrap();

    // TODO: need to configure correct parameters
    let soniton = av_data::audiosample::Soniton {
        bits: 16,
        be: false,
        packed: false,
        planar: false,
        float: false,
        signed: true,
    };
    // let audio_channel_map = av_data::audiosample::ChannelMap::default_map(2);
    let audio_channel_map_mono = av_data::audiosample::ChannelMap::default_map(1);
    let codec_info = av_data::params::AudioInfo {
        rate: 0,
        map: Some(audio_channel_map_mono.clone()),
        format: None,
    };
    let codec_params = av_data::params::CodecParams {
        kind: Some(av_data::params::MediaKind::Audio(codec_info)),
        codec_id: Some(String::from("libopus")),
        extradata: None,
        bit_rate: 0,
        convergence_window: 0,
        delay: 0,
    };

    enc_ctx.set_params(&codec_params).unwrap();
    enc_ctx.configure().unwrap();
    // enc_ctx.set_option("application", "audio").unwrap();

    thread::spawn(move || {
        let mut frame_idx = 0;

        loop {
            let raw_frame = encoder_rx.recv().unwrap();
            // println!("raw sound size: {}", raw_frame.buf.len());

            let now = now_utc();
            let expired = now.sub(FRAME_EXPIRE_DURATION);
            if raw_frame.timestamp < expired {
                println!("raw frame is decayed, dropping.. {:?} < {:?}", raw_frame.timestamp, expired);
                frame_idx += 1;
                continue;
            }

            let av_frame = {
                let time_info = {
                    let mut ti: av_data::timeinfo::TimeInfo = av_data::timeinfo::TimeInfo::default();
                    ti.timebase = Some(av_data::rational::Rational64::new(1, fps));
                    ti.pts = Some(frame_idx);
                    ti
                };

                let frame_info = av_data::frame::AudioInfo {
                    samples: raw_frame.samples,
                    rate: raw_frame.sample_rate,
                    map: audio_channel_map_mono.clone(),
                    format: Arc::new(soniton),
                };

                let frame_kind = av_data::frame::MediaKind::Audio(frame_info.clone());
                let arc_frame = {
                    let mut f = av_data::frame::new_default_frame(frame_kind, Some(time_info));
                    // copy_interleaved_sound_samples(&raw_frame.buf, &mut f);
                    copy_interleaved_sound_samples_mono(&raw_frame.buf, &mut f);
                    Arc::new(f)
                };
                arc_frame
            };

            let encoded_frame = {
                enc_ctx.send_frame(&av_frame).unwrap();
                enc_ctx.flush().unwrap();

                let packet = enc_ctx.receive_packet().unwrap();
                // println!("encoded frame size: {}", packet.data.len());

                EmuEncSoundFrame { buf: packet.data }
            };

            frame_tx.send(encoded_frame).unwrap();

            frame_idx += 1;
        }
    });
}

fn run_sound_handler(props: &GameProperties, frame_rx: channel::Receiver<EmuEncSoundFrame>) {
    let frame_output_path = String::from(&props.soundframe_output);
    assert!(frame_output_path.starts_with("ipc://"));

    thread::spawn(move || {
        let mut socket = Socket::new(Protocol::Push).unwrap();
        socket.bind(&frame_output_path).unwrap();

        loop {
            let frame: EmuEncSoundFrame = frame_rx.recv().unwrap();
            match socket.nb_write(frame.buf.as_ref()) {
                Ok(_) => {
                    // println!("sent frame to nanomsg q: {}", sent);
                },
                Err(_) => {
                    // println!("problem while writing: {}", err);
                }
            }
        }
    });
}

fn run_input_handler(props: &GameProperties, mut emu: (impl Emulator + Send + 'static)) {
    let key_input_path = String::from(&props.key_input);

    fn compose_input_evt_from_buf(b: &[u8]) -> EmuInputEvent {
        let evt_value = atoi(&b[0..3]).unwrap();
        let evt_type = match &b[3] {
            b'd' => InputKind::INPUT_KEY_DOWN,
            b'u' => InputKind::INPUT_KEY_UP,
            _ => InputKind::INPUT_KEY_DOWN,
        };
        EmuInputEvent { value: evt_value, kind: evt_type }
    }

    thread::spawn(move || {
        let mut socket = Socket::new(Protocol::Pull).unwrap();
        socket.bind(&key_input_path).unwrap();

        let mut buf = [0u8; 4];
        loop {
            match socket.nb_read(&mut buf) {
                Ok(bytes_read) => {
                    // println!("read {} bytes", bytes_read);
                    if bytes_read == 4 && buf.len() == 4 {
                        let evt = compose_input_evt_from_buf(&buf);
                        println!("input evt: {:?}", evt);
                        emu.put_input_event(evt);
                    }
                },
                Err(_) => {
                    // println!("problem while reading: {}", err);
                }
            }
        };
    });
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let props = extract_properties_from_args(&args);

    let (image_enc_tx, image_enc_rx) = channel::bounded(CHANNEL_BUF_SIZE);
    let (image_frame_tx, image_frame_rx) = channel::bounded(CHANNEL_BUF_SIZE);

    let (sound_enc_tx, sound_enc_rx) = channel::bounded(CHANNEL_BUF_SIZE);
    let (sound_frame_tx, sound_frame_rx) = channel::bounded(CHANNEL_BUF_SIZE);

    let image_passer = |frame: EmuImageFrame| { image_enc_tx.send(frame).unwrap(); };
    let sound_passer = |frame: EmuSoundFrame| { sound_enc_tx.send(frame).unwrap(); };

    let mut emu = MameEmulator::emulator_instance();
    emu.set_image_frame_info(props.resolution.w, props.resolution.h, props.fps);
    emu.set_image_frame_cb(image_passer);
    emu.set_sound_frame_cb(sound_passer);

    run_frame_encoder(&props, image_enc_rx, image_frame_tx);
    run_frame_handler(&props, image_frame_rx);

    run_sound_encoder(&props, sound_enc_rx, sound_frame_tx);
    run_sound_handler(&props, sound_frame_rx);

    run_input_handler(&props, emu.clone());

    emu.run(&props.system_name);
}