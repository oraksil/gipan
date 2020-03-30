extern crate libemu;

use std::{
    io::{Write, Seek, SeekFrom},
    fs::File,
    env,
};

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
struct GameProperties<'a> {
    system_name: &'a str,
    resolution: Resolution,
}

fn parse_resolution(arg: &String) -> (i32, i32) {
    let whs: Vec<usize> = arg.split("x")
        .map(|s| s.parse().unwrap())
        .collect();

    (whs[0] as i32, whs[1] as i32)
}

fn extract_properties_from_args(args: &Vec<String>) -> GameProperties {
    let mut props = GameProperties::default();
    props.resolution = Resolution::from_size(480, 320);

    for (i, arg) in args.iter().map(|s| { s.as_str() }).enumerate() {
        match arg {
            "--resolution" => {
                let (w, h) = parse_resolution(&args[i+1]);
                props.resolution = Resolution::from_size(w, h);
            },
            "--game" => {
                props.system_name = args[i+1].as_str();
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let game_props = extract_properties_from_args(&args);

    let mut f = File::create("./frames.raw").expect("failed to open frames.raw");
    let frame_writer = |frame: &Frame| {
        let _ = f.seek(SeekFrom::Start(0));
        let _ = f.write(frame.image_buf.as_ref());
    };

    let mut emu = MameEmulator::emulator_instance();
    emu.set_frame_info(game_props.resolution.w, game_props.resolution.h);
    emu.set_frame_callback(frame_writer);
    emu.run(game_props.system_name);
}