#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused_imports)]

use std::slice;
use std::time::Instant;
use libc::*;
use bytes::Bytes;

include!("./bindings.rs");

pub enum InputKind {
    INPUT_KEY_UP,
    INPUT_KEY_DOWN,
}

pub struct InputEvent {
    value: u8,
    kind: InputKind,
}

pub struct Frame {
    pub image_buf: Bytes
}

pub trait Emulator {
    fn set_frame_info(&mut self, w: i32, h: i32);
    fn set_frame_callback(&mut self, callback: impl FnMut(&Frame));
    fn put_input_event(&self, event: &InputEvent);
    fn run(&self, system_name: &str) -> i32;
}

pub struct MameEmulator {
    mame_inst: *mut mame_t,
}

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

    fn set_frame_callback(&mut self, mut callback: impl FnMut(&Frame)) {
        mame_register_callback(
            self.mame_inst,
            move |raw_frame: mame_frame_t| {
                let buf = unsafe { slice::from_raw_parts(raw_frame.buffer, raw_frame.buf_size) };
                let frame = Frame {
                    image_buf: Bytes::from(buf),
                };
                callback(&frame);
            }
        );
    }

    fn put_input_event(&self, event: &InputEvent) {

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