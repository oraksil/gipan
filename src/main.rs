extern crate libemu;
extern crate libenc;

use std::{env, thread};
use std::time::Instant;
use std::sync::{Arc, RwLock};

use atoi::atoi;
use nanomsg::{Socket, Protocol};
use crossbeam_channel as channel;

use libemu::Emulator;
use libenc::Encoder;

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
    keyframe_interval: usize,
    system_name: String,
    imageframe_output: String,
    soundframe_output: String,
    key_input: String,
    idle_time_to_enc_sleep: i32,        // in secs
}

struct GameContext {
    last_key_input_ts: Instant,
}

impl GameContext {
    fn is_key_input_in_idle(&self, threshold: i32) -> bool {
        if threshold <= 0 {
            false
        } else {
            let elapsed = self.last_key_input_ts.elapsed();
            elapsed.as_secs() > threshold as u64
        }
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
            "--idle-time-to-enc-sleep" => {
                props.idle_time_to_enc_sleep = next_arg().parse().unwrap()
            }
            _ => {
                if arg.starts_with("--") {
                    panic!("invalid args have been passed");
                }
            }
        }
    }

    props
}

fn run_frame_encoder(
    props: &GameProperties,
    encoder_rx: channel::Receiver<libemu::EmuImageFrame>,
    frame_tx: channel::Sender<libenc::EncodedFrame>) {

    let mut vid_enc = libenc::H264Encoder::create(
        props.resolution.w,
        props.resolution.h,
        props.fps,
        props.keyframe_interval);
    
    thread::spawn(move || {
        loop {
            let raw_frame = encoder_rx.recv().unwrap();
            // println!("raw frame size: {}", raw_frame.buf.len());

            let frame = libenc::VideoFrame::from(&raw_frame.buf, raw_frame.timestamp);
            match vid_enc.encode_video(&frame) {
                Ok(encoded) => {
                    frame_tx.send(encoded).unwrap();
                },
                Err(msg) => {
                    println!("{}", msg);
                }
            }

        }
    });
}

fn run_frame_handler(
    props: &GameProperties,
    frame_rx: channel::Receiver<libenc::EncodedFrame>) {
        
    let frame_output_path = String::from(&props.imageframe_output);
    assert!(frame_output_path.starts_with("ipc://"));

    thread::spawn(move || {
        let mut socket = Socket::new(Protocol::Push).unwrap();
        socket.set_send_buffer_size(4096 * 1024).unwrap();
        socket.bind(&frame_output_path).unwrap();

        loop {
            let frame = frame_rx.recv().unwrap();
            match socket.nb_write(frame.buf.as_ref()) {
                Ok(_) => {
                    // println!("sent frame to nanomsg q: {}", sent);
                },
                Err(err) => {
                    println!("problem while writing: {}", err);
                }
            }
        }
    });
}

fn run_sound_encoder(
    props: &GameProperties,
    encoder_rx: channel::Receiver<libemu::EmuSoundFrame>,
    frame_tx: channel::Sender<libenc::EncodedFrame>) {

    let mut opus_enc = libenc::OpusEncoder::create(props.fps);

    thread::spawn(move || {
        loop {
            let raw_frame = encoder_rx.recv().unwrap();
            // println!("raw sound size: {}", raw_frame.buf.len());

            let frame = libenc::AudioFrame::from(
                &raw_frame.buf, raw_frame.timestamp, raw_frame.samples, raw_frame.sample_rate);

            match opus_enc.encode_audio(&frame) {
                Ok(encoded) => {
                    frame_tx.send(encoded).unwrap();
                },
                Err(msg) => {
                    println!("{}", msg);
                }
            }

        }
    });
}

fn run_sound_handler(
    props: &GameProperties,
    frame_rx: channel::Receiver<libenc::EncodedFrame>) {

    let frame_output_path = String::from(&props.soundframe_output);
    assert!(frame_output_path.starts_with("ipc://"));

    thread::spawn(move || {
        let mut socket = Socket::new(Protocol::Push).unwrap();
        socket.bind(&frame_output_path).unwrap();

        loop {
            let frame = frame_rx.recv().unwrap();
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

fn run_input_handler(
    props: &GameProperties,
    ctx: Arc<RwLock<GameContext>>,
    mut emu: (impl libemu::Emulator + Send + 'static)) {

    let key_input_path = String::from(&props.key_input);

    fn compose_input_evt_from_buf(b: &[u8]) -> libemu::EmuInputEvent {
        let evt_value = atoi(&b[0..3]).unwrap();
        let evt_type = match &b[3] {
            b'd' => libemu::InputKind::INPUT_KEY_DOWN,
            b'u' => libemu::InputKind::INPUT_KEY_UP,
            _ => libemu::InputKind::INPUT_KEY_DOWN,
        };
        libemu::EmuInputEvent { value: evt_value, kind: evt_type }
    }

    let update_last_key_input_ts = move || {
        if let Ok(mut c) = ctx.write() {
            (*c).last_key_input_ts = Instant::now();
        }
    };

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

                        update_last_key_input_ts();
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
    let ctx = Arc::new(RwLock::new(GameContext { last_key_input_ts: Instant::now(), }));

    let (img_enc_tx, img_enc_rx) = channel::bounded(CHANNEL_BUF_SIZE);
    let (img_frame_tx, img_frame_rx) = channel::bounded(CHANNEL_BUF_SIZE);

    let (snd_enc_tx, snd_enc_rx) = channel::bounded(CHANNEL_BUF_SIZE);
    let (snd_frame_tx, snd_frame_rx) = channel::bounded(CHANNEL_BUF_SIZE);

    let mut emu = libemu::MameEmulator::create(
        props.resolution.w,
        props.resolution.h,
        props.fps);

    let is_idle = || {
        if let Ok(c) = ctx.read() {
            (*c).is_key_input_in_idle(props.idle_time_to_enc_sleep)
        } else {
            false
        }
    };
    emu.set_image_frame_cb(|f: libemu::EmuImageFrame| { 
        if !is_idle() { img_enc_tx.send(f).unwrap(); }
    });
    emu.set_sound_frame_cb(|f: libemu::EmuSoundFrame| {
        if !is_idle() { snd_enc_tx.send(f).unwrap(); }
    });

    run_frame_encoder(&props, img_enc_rx, img_frame_tx);
    run_frame_handler(&props, img_frame_rx);

    run_sound_encoder(&props, snd_enc_rx, snd_frame_tx);
    run_sound_handler(&props, snd_frame_rx);

    run_input_handler(&props, ctx.clone(), emu.clone());

    emu.run(&props.system_name);
}