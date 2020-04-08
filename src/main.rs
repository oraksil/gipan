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
const TIMEBASE: i64 = 60000;

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
    system_name: String,
    frame_output: String,
    key_input: String,
}

impl GameProperties {
    fn max_frame_buffer_size(&self) -> usize {
        let frame_cap = 10;
        (self.resolution.w * self.resolution.h * COLOR_DEPTH * frame_cap) as usize
    }
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
    let fps = props.fps;

    let yuv420: Formaton = *YUV420;
    let codec_video_info = av_data::params::VideoInfo {
        width: r.w,
        height: r.h,
        format: Some(Arc::new(yuv420)),
    };
    let frame_video_info = av_data::frame::VideoInfo {
        pic_type: av_data::frame::PictureType::I,
        width: r.w,
        height: r.h,
        format: Arc::new(yuv420),
    };
    let codec_params = av_data::params::CodecParams {
        kind: Some(av_data::params::MediaKind::Video(codec_video_info)),
        codec_id: Some(String::from("vpx")),
        extradata: None,
        bit_rate: 256 * 1024,
        convergence_window: 0,
        delay: 0,
    };

    let encoder_list = av_codec::encoder::Codecs::from_list(&[VP9_DESCR]);
    let mut enc_ctx = av_codec::encoder::Context::by_name(&encoder_list, &"vp9").unwrap();

    enc_ctx.set_params(&codec_params).unwrap();
    enc_ctx.configure().unwrap();
    enc_ctx.set_option("cpu-used", 2u64).unwrap();
    // enc_ctx.set_option("qmin", 0u64).unwrap();
    // enc_ctx.set_option("qmax", 0u64).unwrap();

    thread::spawn(move || {
        let yuv_size = r.w * r.h / COLOR_DEPTH;
        let chroma_size = yuv_size / 4;

        let timescale = TIMEBASE / fps as i64;
        let mut frame_idx = 0;

        loop {
            let raw_frame = encoder_rx.recv().unwrap();
            println!("raw frame size: {}", raw_frame.image_buf.len());

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
                    ti.timebase = Some(av_data::rational::Rational64::new(TIMEBASE, 1));
                    ti.pts = Some(timescale * frame_idx);
                    ti
                };
                frame_idx += 1;

                let arc_frame = {
                    let mut f = av_data::frame::new_default_frame(
                        av_data::frame::MediaKind::Video(frame_video_info.clone()),
                        Some(time_info)
                    );
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

                EmuFrame { image_buf: buf }
            };

            frame_tx.send(encoded_frame).unwrap();
        }
    });
}

fn run_frame_handler(props: &GameProperties, frame_rx: Receiver<EmuFrame>) {
    let frame_buf_size = props.max_frame_buffer_size();
    let frame_output_path = String::from(&props.frame_output);

    if frame_output_path.starts_with("ipc://") {
        thread::spawn(move || {
            let mut socket = Socket::new(Protocol::Push).unwrap();
            socket.set_send_buffer_size(frame_buf_size).unwrap();
            socket.bind(&frame_output_path).unwrap();

            loop {
                let frame: EmuFrame = frame_rx.recv().unwrap();
                match socket.nb_write(frame.image_buf.as_ref()) {
                    Ok(sent) => {
                        println!("sending frame to nanomsg q: {}", sent);
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
                println!("{}", sent);
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

    let ch_buf_size = props.max_frame_buffer_size();
    let (encode_tx, encode_rx) = mpsc::sync_channel::<EmuFrame>(ch_buf_size);
    let (frame_tx, frame_rx) = mpsc::sync_channel::<EmuFrame>(ch_buf_size);

    let mut emu = MameEmulator::emulator_instance();
    emu.set_frame_info(props.resolution.w, props.resolution.h);

    let frame_passer = |frame: EmuFrame| { encode_tx.send(frame).unwrap(); };
    emu.set_frame_callback(frame_passer);

    run_frame_encoder(&props, encode_rx, frame_tx);
    run_frame_handler(&props, frame_rx);
    run_input_handler(&props, emu.clone());

    emu.run(&props.system_name);
}