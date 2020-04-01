extern crate libemu;

use std::{
    io::{Write, Seek, SeekFrom},
    fs::File,
    env,
    sync::mpsc::{Sender, Receiver},
    sync::mpsc,
    thread,
};

use nanomsg::{Socket, Protocol, Error};

use libemu::{Emulator, Frame, MameEmulator};

#[derive(Debug, Default)]
struct Resolution {
    w: i32,
    h: i32,
}

impl Resolution {
    fn from_size(w: i32, h: i32) -> Resolution {
        Resolution { w, h }
    }
}

#[derive(Debug, Default)]
struct GameProperties {
    resolution: Resolution,
    fps: i32,
    system_name: String,
    frame_output: String,
}

impl GameProperties {
    fn max_frame_buffer_size(&self) -> usize {
        let color_depth = 4;
        (self.resolution.w * self.resolution.h * color_depth * self.fps) as usize
    }
}

fn parse_resolution(arg: String) -> (i32, i32) {
    let whs: Vec<usize> = arg.split("x")
        .map(|s| s.parse().unwrap())
        .collect();

    (whs[0] as i32, whs[1] as i32)
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

fn channel<T>() -> (Sender<T>, Receiver<T>) {
    mpsc::channel()
}

fn choose_frame_handler(props: &GameProperties, rx: Receiver<Frame>) -> Box<dyn FnMut() + Send> {
    let frame_buf_size = props.max_frame_buffer_size();
    let frame_output_path = String::from(&props.frame_output);

    if frame_output_path.starts_with("ipc://") {
        Box::new(move || {
            let mut socket = Socket::new(Protocol::Push).unwrap();
            socket.set_send_buffer_size(frame_buf_size).unwrap();
            socket.connect(&frame_output_path).unwrap();

            loop {
                let frame: Frame = rx.recv().unwrap();
                let sent = socket.write(frame.image_buf.as_ref()).unwrap();
                println!("{}", sent);
            }
        })
    } else {
        Box::new(move || {
            let mut f = File::create("./frames.raw").expect("failed to open frames.raw");
            loop {
                let frame: Frame = rx.recv().unwrap();
                f.seek(SeekFrom::Start(0)).unwrap();
                let sent = f.write(frame.image_buf.as_ref()).unwrap();
                println!("{}", sent);
            }
        })
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let props = extract_properties_from_args(&args);

    let (frame_ch_tx, frame_ch_rx) = channel::<Frame>();

    let frame_passer = |frame: Frame| { frame_ch_tx.send(frame).unwrap(); };
    let frame_handler = choose_frame_handler(&props, frame_ch_rx);
    thread::spawn(frame_handler);

    let mut emu = MameEmulator::emulator_instance();
    emu.set_frame_info(props.resolution.w, props.resolution.h);
    emu.set_frame_callback(frame_passer);
    emu.run(&props.system_name);
}