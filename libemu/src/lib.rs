#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused_imports)]

use std::slice;
use libc::*;
use bytes::Bytes;

include!("./bindings.rs");

#[derive(Debug)]
pub enum InputKind {
    INPUT_KEY_DOWN,
    INPUT_KEY_UP,
}

#[derive(Debug)]
pub struct EmuInputEvent {
    pub value: u8,
    pub kind: InputKind,
}

pub struct EmuFrame {
    pub image_buf: Bytes
}

pub trait Emulator: Clone + Send {
    fn set_frame_info(&mut self, w: i32, h: i32);
    fn set_frame_callback(&mut self, callback: impl FnMut(EmuFrame));
    fn put_input_event(&mut self, event: EmuInputEvent);
    fn run(&self, system_name: &str) -> i32;
}

#[derive(Clone)]
pub struct MameEmulator {
    mame_inst: *mut mame_t,
}

unsafe impl Send for MameEmulator {}

impl MameEmulator {
    pub fn emulator_instance() -> impl Emulator {
        let mame_inst: *mut mame_t = unsafe { get_mame_instance() };
        MameEmulator {
            mame_inst: mame_inst,
        }
    }
}

// https://blog.seantheprogrammer.com/neat-rust-tricks-passing-rust-closures-to-c
fn mame_register_callback<F>(mame: *mut mame_t, callback: F)
where F: FnMut(mame_frame_t), {
    let ctx = Box::into_raw(Box::new(callback));
    unsafe {
        match (*mame).set_frame_cb {
            Some(f) => f(ctx as *mut _, Some(mame_call_closure::<F>)),
            None => panic!("set_frame_cb method is not implemented.")
        }
    }
}

unsafe extern "C" fn mame_call_closure<F>(ctx: *mut libc::c_void, frame: mame_frame_t)
where F: FnMut(mame_frame_t), {
    let callback_ptr = ctx as *mut F;
    let callback = &mut *callback_ptr;
    callback(frame);
}

impl Emulator for MameEmulator {
    fn set_frame_info(&mut self, w: i32, h: i32) {
        unsafe {
            match (*self.mame_inst).set_frame_info {
                Some(f) => f(w, h),
                None => panic!("set_frame_info method is not implemented.")
            }
        }
    }

    fn set_frame_callback(&mut self, mut callback: impl FnMut(EmuFrame)) {
        mame_register_callback(
            self.mame_inst,
            move |raw_frame: mame_frame_t| {
                let buf = unsafe {
                    slice::from_raw_parts(raw_frame.buffer, raw_frame.buf_size)
                };
                callback(EmuFrame { image_buf: Bytes::from(buf) });
            }
        );
    }

    fn put_input_event(&mut self, event: EmuInputEvent) {
        let mame_input = mame_input_event_t {
            key: event.value,
            type_: match event.kind {
                InputKind::INPUT_KEY_UP => mame_input_enum_t_INPUT_KEY_UP,
                InputKind::INPUT_KEY_DOWN => mame_input_enum_t_INPUT_KEY_DOWN,
            }
        };
        unsafe {
            match (*self.mame_inst).enqueue_input_event {
                Some(f) => f(mame_input),
                None => panic!("enqueue_input_event is not implemented.")
            }
        }
    }

    fn run(&self, system_name: &str) -> i32 {
        let sys_name = String::from(system_name);
        unsafe {
            match (*self.mame_inst).run {
                Some(f) => f(sys_name.as_ptr() as *const c_char),
                None => panic!("run method is not implemented.")
            }
        }
    }
}