extern crate libemu;

use std::{
    io::{Read, Write, Seek, SeekFrom},
    fs::File,
    env,
    sync::mpsc::{SyncSender, Sender, Receiver},
    sync::mpsc,
    thread,
};

use atoi::atoi;

use bytes::BytesMut;

use nanomsg::{
    Socket,
    Protocol,
};

use libemu::{
    Emulator,
    MameEmulator,
    Frame,
    InputEvent,
    InputKind,
};

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
    key_input: String,
}

impl GameProperties {
    fn max_frame_buffer_size(&self) -> usize {
        let color_depth = 4;
        let frame_cap = 10;
        (self.resolution.w * self.resolution.h * color_depth * frame_cap) as usize
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

fn channel<T>(buf_size: usize) -> (SyncSender<T>, Receiver<T>) {
    mpsc::sync_channel::<T>(buf_size)
}

fn run_frame_handler(props: &GameProperties, rx: Receiver<Frame>) {
    let frame_buf_size = props.max_frame_buffer_size();
    let frame_output_path = String::from(&props.frame_output);

    if frame_output_path.starts_with("ipc://") {
        thread::spawn(move || {
            let mut socket = Socket::new(Protocol::Push).unwrap();
            socket.set_send_buffer_size(frame_buf_size).unwrap();
            socket.bind(&frame_output_path).unwrap();

            loop {
                let frame: Frame = rx.recv().unwrap();
                match socket.nb_write(frame.image_buf.as_ref()) {
                    Ok(sent) => {
                        // println!("{}", sent);
                    },
                    Err(err) => {
                        // println!("problem while reading: {}", err);
                    }
                }
            }
        });
    }
    else {
        thread::spawn(move || {
            let mut f = File::create("./frames.raw").expect("failed to open frames.raw");
            loop {
                let frame: Frame = rx.recv().unwrap();
                f.seek(SeekFrom::Start(0)).unwrap();
                let sent = f.write(frame.image_buf.as_ref()).unwrap();
                println!("{}", sent);
            }
        });
    }
}

fn run_input_handler(props: &GameProperties, mut emu: (impl Emulator + Send + 'static)) {
    let key_input_path = String::from(&props.key_input);

    fn compose_input_evt_from_buf(b: &[u8]) -> InputEvent {
        let evt_value = atoi(&b[0..3]).unwrap();
        let evt_type = match &b[3] {
            b'd' => InputKind::INPUT_KEY_DOWN,
            b'u' => InputKind::INPUT_KEY_UP,
            _ => InputKind::INPUT_KEY_DOWN,
        };
        InputEvent { value: evt_value, kind: evt_type }
    }

    thread::spawn(move || {
        let mut socket = Socket::new(Protocol::Pull).unwrap();
        socket.bind(&key_input_path).unwrap();

        let mut buf = [0u8; 4];
        loop {
            match socket.nb_read(&mut buf) {
                Ok(count) => {
                    println!("read {} bytes", count);
                    if count == 4 && buf.len() == 4 {
                        let evt = compose_input_evt_from_buf(&buf);
                        println!("input evt: {:?}", evt);
                        emu.put_input_event(evt);
                    }
                },
                Err(err) => {
                    // println!("problem while reading: {}", err);
                }
            }
        };
    });
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let props = extract_properties_from_args(&args);

    let (frame_ch_tx, frame_ch_rx) = channel::<Frame>(props.max_frame_buffer_size());
    let frame_passer = |frame: Frame| { frame_ch_tx.send(frame).unwrap(); };

    let mut emu = MameEmulator::emulator_instance();
    emu.set_frame_info(props.resolution.w, props.resolution.h);
    emu.set_frame_callback(frame_passer);

    run_frame_handler(&props, frame_ch_rx);
    run_input_handler(&props, emu.clone());

    emu.run(&props.system_name);
}