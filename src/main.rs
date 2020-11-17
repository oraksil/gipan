extern crate libemu;

extern crate serde;

use std::io::{Read, Write};
use std::{env, thread, str};

use nanomsg::{Socket, Protocol};
use crossbeam_channel as channel;

use libemu::Emulator;
use libenc::Encoder;

use serde::{Deserialize};

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
    cmd_input: String,
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
    props.imageframe_output = String::from("ipc://./images.ipc");
    props.soundframe_output = String::from("ipc://./sounds.ipc");
    props.cmd_input = String::from("ipc://./cmds.ipc");

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
            "--cmd-input" => {
                props.cmd_input = next_arg()
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
                Err(_) => {
                    // println!("{}", msg);
                }
            }

        }
    });
}

fn run_frame_handler(
    props: &GameProperties,
    frame_rx: channel::Receiver<libenc::EncodedFrame>) {
        
    let frame_output_path = String::from(&props.imageframe_output);

    thread::spawn(move || {
        let mut socket = Socket::new(Protocol::Push).unwrap();
        socket.set_send_buffer_size(4096 * 1024).unwrap();
        socket.bind(&frame_output_path).unwrap();

        loop {
            let frame = frame_rx.recv().unwrap();
            socket.write_all(frame.buf.as_ref()).unwrap();
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
                Err(_) => {
                    // println!("{}", msg);
                }
            }

        }
    });
}

fn run_sound_handler(
    props: &GameProperties,
    frame_rx: channel::Receiver<libenc::EncodedFrame>) {

    let frame_output_path = String::from(&props.soundframe_output);

    thread::spawn(move || {
        let mut socket = Socket::new(Protocol::Push).unwrap();
        socket.bind(&frame_output_path).unwrap();

        loop {
            let frame = frame_rx.recv().unwrap();
            socket.write_all(frame.buf.as_ref()).unwrap();
        }
    });
}

// Command Specification
// 'key'
//   - args[0]: string of key input (ex, 053d) 
// 'ctrl'
//   - args[0]: string for stream control (ex, pause / resume)
#[derive(Deserialize, Debug)]
struct Command {
  cmd: String,
  args: Vec<String>,
}

fn run_cmd_handler(
    props: &GameProperties,
    emu: (impl libemu::Emulator + Send + 'static)) {

    let cmd_input_path = String::from(&props.cmd_input);

    thread::spawn(move || {
        let handle_cmd_key = |args: &Vec<String>| {
            // parse buf and put input to emu
            let input_str = &args[0];
            emu.put_input_event(libemu::EmuInputEvent {
                value: input_str[0..3].parse::<u8>().unwrap(),
                kind: match &input_str[3..] {
                    "d" => libemu::InputKind::INPUT_KEY_DOWN,
                    "u" => libemu::InputKind::INPUT_KEY_UP,
                    _ => libemu::InputKind::INPUT_KEY_DOWN,
                }
            });
        };

        let handle_cmd_ctrl = |args: &Vec<String>| {
            let ctrl_val = &args[0];
            match &ctrl_val[..] {
                "pause" => emu.pause(),
                "resume" => emu.resume(),
                _ => println!("ctrl val: {}", &args[0]),
            }
        };

        // connect to cmd input queue, then polling and handling cmd
        let mut socket = Socket::new(Protocol::Pull).unwrap();
        socket.bind(&cmd_input_path).unwrap();

        let mut buf = [0u8; 1024];
        loop {
            let bytes_read = socket.read(&mut buf).unwrap();
            let json_str = str::from_utf8(&buf[0..bytes_read]).unwrap();
            let command: Command = serde_json::from_str(&json_str).unwrap();

            match &command.cmd[..] {
                "key" => handle_cmd_key(&command.args),
                "ctrl" => handle_cmd_ctrl(&command.args),
                _ => println!("not supported cmd"),
            }
        };
    });
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let props = extract_properties_from_args(&args);

    let mut emu = libemu::MameEmulator::create(
        props.resolution.w,
        props.resolution.h,
        props.fps);

    let (img_enc_tx, img_enc_rx) = channel::bounded(CHANNEL_BUF_SIZE);
    let (img_frame_tx, img_frame_rx) = channel::bounded(CHANNEL_BUF_SIZE);
    emu.set_image_frame_cb(|f: libemu::EmuImageFrame| { img_enc_tx.send(f).unwrap(); });
    run_frame_encoder(&props, img_enc_rx, img_frame_tx);
    run_frame_handler(&props, img_frame_rx);

    let (snd_enc_tx, snd_enc_rx) = channel::bounded(CHANNEL_BUF_SIZE);
    let (snd_frame_tx, snd_frame_rx) = channel::bounded(CHANNEL_BUF_SIZE);
    emu.set_sound_frame_cb(|f: libemu::EmuSoundFrame| { snd_enc_tx.send(f).unwrap(); });
    run_sound_encoder(&props, snd_enc_rx, snd_frame_tx);
    run_sound_handler(&props, snd_frame_rx);

    run_cmd_handler(&props, emu.clone());

    emu.run(&props.system_name);
}
