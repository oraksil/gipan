extern crate libemu;

mod utils;

use std::{
    io::{Write, Seek, SeekFrom},
    fs::File,
    env,
    sync::mpsc::{SyncSender, Receiver},
    sync::mpsc,
    sync::Arc,
    thread,
    ops::Sub,
    time::{SystemTime, Duration, UNIX_EPOCH},
};

use atoi::atoi;
use bytes::{Bytes, BytesMut};

use nanomsg::{Socket, Protocol};

use av_data;
use av_data::pixel::formats::YUV420;
use av_data::pixel::Formaton;
use av_codec;
use av_codec::common::CodecList;

use libvpx::encoder::VP9_DESCR;

use libemu::{
    Emulator,
    MameEmulator,
    EmuFrame,
    EmuInputEvent,
    InputKind,
};

const COLOR_DEPTH: usize = 4;
const FRAME_EXPIRE_DURATION: Duration = Duration::from_millis(100);
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
    fps: i32,
    keyframe_interval: i32,
    system_name: String,
    frame_output: String,
    key_input: String,
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
    props.frame_output = String::from("ipc://./frames.ipc");

    for (i, arg) in args.iter().map(|s| s.as_str()).enumerate() {
        let next_arg = || { args[i+1].clone() };
        match arg {
            "--game" => {
                props.system_name = next_arg()
            }
            "--frame-output" => {
                props.frame_output = next_arg()
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

fn run_frame_encoder(props: &GameProperties, encoder_rx: Receiver<EmuFrame>, frame_tx: SyncSender<EmuFrame>) {
    let r = props.resolution;
    let fps = props.fps as i64;
    let kf_interval = props.keyframe_interval as i64;

    const YUV_FMT_420: Formaton = *YUV420;
    let codec_video_info = av_data::params::VideoInfo {
        width: r.w,
        height: r.h,
        format: Some(Arc::new(YUV_FMT_420)),
    };
    let i_frame_video_info = av_data::frame::VideoInfo {
        pic_type: av_data::frame::PictureType::I,
        width: r.w,
        height: r.h,
        format: Arc::new(YUV_FMT_420),
    };
    let b_frame_video_info = av_data::frame::VideoInfo {
        pic_type: av_data::frame::PictureType::B,
        width: r.w,
        height: r.h,
        format: Arc::new(YUV_FMT_420),
    };
    let codec_params = av_data::params::CodecParams {
        kind: Some(av_data::params::MediaKind::Video(codec_video_info)),
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
    enc_ctx.set_option("qmin", 0u64).unwrap();
    enc_ctx.set_option("qmax", 0u64).unwrap();

    thread::spawn(move || {
        let yuv_size = r.w * r.h / COLOR_DEPTH;
        let chroma_size = yuv_size / 4;

        // Regarding timebase and pts,
        // https://stackoverflow.com/questions/43333542/what-is-video-timescale-timebase-or-timestamp-in-ffmpeg/43337235
        let mut frame_idx = 0;
        let mut encoded_frame_cnt = 0;

        loop {
            let raw_frame = encoder_rx.recv().unwrap();
            println!("raw frame size: {}", raw_frame.image_buf.len());

            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let expired = now.sub(FRAME_EXPIRE_DURATION);
            if raw_frame.timestamp < expired {
                println!("raw frame is decayed, dropping.. {:?} < {:?}", raw_frame.timestamp, expired);
                frame_idx += 1;
                continue;
            }

            let mut y = BytesMut::with_capacity(yuv_size);
            let mut u = BytesMut::with_capacity(chroma_size);
            let mut v = BytesMut::with_capacity(chroma_size);
            utils::bgra_to_yuv420(r.w, &raw_frame.image_buf, &mut y, &mut u, &mut v);
            println!("yuv frame size: y: {}, u: {}, v: {}", y.len(), u.len(), v.len());

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
                    av_data::frame::MediaKind::Video(i_frame_video_info.clone())
                } else {
                    av_data::frame::MediaKind::Video(b_frame_video_info.clone())
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
                let buf = Bytes::from(packet.data);
                println!("encoded frame size: {}", buf.len());

                EmuFrame { image_buf: buf, timestamp: raw_frame.timestamp }
            };

            frame_tx.send(encoded_frame).unwrap();

            frame_idx += 1;
            encoded_frame_cnt += 1;
        }
    });
}

fn run_frame_handler(props: &GameProperties, frame_rx: Receiver<EmuFrame>) {
    let frame_output_path = String::from(&props.frame_output);

    if frame_output_path.starts_with("ipc://") {
        thread::spawn(move || {
            let mut socket = Socket::new(Protocol::Push).unwrap();
            socket.bind(&frame_output_path).unwrap();

            loop {
                let frame: EmuFrame = frame_rx.recv().unwrap();
                match socket.nb_write(frame.image_buf.as_ref()) {
                    Ok(sent) => {
                        println!("sent frame to nanomsg q: {}", sent);
                    },
                    Err(_) => {
                        // println!("problem while writing: {}", err);
                    }
                }
            }
        });
    }
    else {
        thread::spawn(move || {
            let mut f = File::create("./frames.raw").expect("failed to open frames.raw");
            loop {
                let frame: EmuFrame = frame_rx.recv().unwrap();
                f.seek(SeekFrom::Start(0)).unwrap();
                let sent = f.write(frame.image_buf.as_ref()).unwrap();
                println!("wrote frame to file: {}", sent);
            }
        });
    }
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
                    println!("read {} bytes", bytes_read);
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

    let (encode_tx, encode_rx) = mpsc::sync_channel::<EmuFrame>(CHANNEL_BUF_SIZE);
    let (frame_tx, frame_rx) = mpsc::sync_channel::<EmuFrame>(CHANNEL_BUF_SIZE);

    let mut emu = MameEmulator::emulator_instance();
    emu.set_frame_info(props.resolution.w, props.resolution.h);

    let frame_passer = |frame: EmuFrame| { encode_tx.send(frame).unwrap(); };
    emu.set_frame_callback(frame_passer);

    run_frame_encoder(&props, encode_rx, frame_tx);
    run_frame_handler(&props, frame_rx);
    run_input_handler(&props, emu.clone());

    emu.run(&props.system_name);
}