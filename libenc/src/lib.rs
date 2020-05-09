use std::ptr;
use std::sync::Arc;
use std::ops::Sub;
use std::time::Duration;

use av_data;
use av_codec;
use av_codec::common::CodecList;

use libvpx::encoder::VP9_DESCR;
use libopus::encoder::OPUS_DESCR;
use x264;

mod utils;

const FRAME_EXPIRE_DURATION: Duration = Duration::from_millis(30);

pub struct VideoFrame {
    pub buf: Vec<u8>,
    pub timestamp: Duration,
}

impl VideoFrame {
    pub fn from(buf: &[u8], timestamp: Duration) -> VideoFrame {
        VideoFrame {
            buf: Vec::from(buf),
            timestamp: timestamp,
        }
    }
}

pub struct AudioFrame {
    buf: Vec<i16>,
    timestamp: Duration,
    samples: usize,
    sample_rate: usize,
}

impl AudioFrame {
    pub fn from(buf: &[i16], timestamp: Duration, samples: usize, sample_rate: usize) -> AudioFrame {
        AudioFrame {
            buf: Vec::from(buf),
            timestamp: timestamp,
            samples: samples,
            sample_rate: sample_rate,
        }
    }
}

pub struct EncodedFrame {
    pub buf: Vec<u8>,
    pub timestamp: Duration,
}

pub trait Encoder {
    fn encode_video(&mut self, frame: &VideoFrame) -> Result<EncodedFrame, String>;
    fn encode_audio(&mut self, frame: &AudioFrame) -> Result<EncodedFrame, String>;
}

pub struct Vp9Encoder {
    w: usize,
    h: usize,
    fps: usize,
    keyframe_interval: usize,

    enc_ctx: av_codec::encoder::Context,

    frame_index: i64,
    encoded_frame_count: i64,
}

impl Vp9Encoder {
    pub fn create(w: usize, h: usize, fps: usize, keyframe_interval: usize) -> impl Encoder {
        Vp9Encoder {
            w: w,
            h: h,
            fps: fps,
            keyframe_interval: keyframe_interval,
            enc_ctx: Vp9Encoder::create_ctx(w, h),
            frame_index: 0,
            encoded_frame_count: 0
        }
    }

    fn frame_info(&self, pic_type: av_data::frame::PictureType) -> av_data::frame::VideoInfo {
        av_data::frame::VideoInfo {
            pic_type: pic_type,
            width: self.w,
            height: self.h,
            format: Arc::new(*av_data::pixel::formats::YUV420),
        }
    }

    fn create_ctx(w: usize, h: usize) -> av_codec::encoder::Context {
        let codec_info = av_data::params::VideoInfo {
            width: w,
            height: h,
            format: Some(Arc::new(*av_data::pixel::formats::YUV420)),
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

        enc_ctx
    }
}

impl Encoder for Vp9Encoder {
    fn encode_audio(&mut self, _frame: &AudioFrame) -> Result<EncodedFrame, String> {
        unimplemented!();
    }

    fn encode_video(&mut self, frame: &VideoFrame) -> Result<EncodedFrame, String> {
        // skip encoding for expired frame
        let now = utils::time::now_utc();
        let expired = now.sub(FRAME_EXPIRE_DURATION);
        if frame.timestamp < expired {
            self.frame_index += 1;
            return Err(format!("raw frame is decayed, dropping.. {:?} < {:?}", frame.timestamp, expired));
        }

        let yuv_size = self.w * self.h;
        let chroma_size = yuv_size / 4;

        // Regarding timebase and pts,
        // https://stackoverflow.com/questions/43333542/what-is-video-timescale-timebase-or-timestamp-in-ffmpeg/43337235
        let mut out_y = vec![0u8; yuv_size];
        let mut out_u = vec![0u8; chroma_size];
        let mut out_v = vec![0u8; chroma_size];
        utils::converter::bgra_to_yuv420(
            self.w, self.h, &frame.buf, out_y.as_mut(), out_u.as_mut(), out_v.as_mut());
        // println!("yuv frame size: y: {}, u: {}, v: {}", y.len(), u.len(), v.len());

        let yuv_bufs = [out_y, out_u, out_v];
        let source = yuv_bufs.iter().map(|v| v.as_ref());

        // https://stackoverflow.com/questions/13286022/can-anyone-help-in-understanding-avframe-linesize
        let yuv_strides = [self.w, self.w / 2, self.w / 2];
        let linesizes = yuv_strides.iter().map(|v| *v);

        let av_frame = {
            let time_info = {
                let mut ti: av_data::timeinfo::TimeInfo = av_data::timeinfo::TimeInfo::default();
                ti.timebase = Some(av_data::rational::Rational64::new(1, self.fps as i64));
                ti.pts = Some(self.frame_index);
                ti
            };

            let frame_kind = if self.encoded_frame_count % (self.keyframe_interval as i64) == 0 {
                av_data::frame::MediaKind::Video(self.frame_info(av_data::frame::PictureType::I))
            } else {
                av_data::frame::MediaKind::Video(self.frame_info(av_data::frame::PictureType::P))
            };
            let arc_frame = {
                let mut f = av_data::frame::new_default_frame(frame_kind, Some(time_info));
                f.copy_from_slice(source, linesizes);
                Arc::new(f)
            };
            arc_frame
        };

        let encoded_packet = {
            self.enc_ctx.send_frame(&av_frame).unwrap();
            self.enc_ctx.flush().unwrap();
            self.enc_ctx.receive_packet().unwrap()
        };

        self.frame_index += 1;
        self.encoded_frame_count += 1;

        Ok(EncodedFrame {
            buf: encoded_packet.data,
            timestamp: frame.timestamp,
        })
    }
}

pub struct OpusEncoder {
    fps: usize,

    enc_ctx: av_codec::encoder::Context,

    frame_index: i64,
}

impl OpusEncoder {
    pub fn create(fps: usize) -> impl Encoder {
        OpusEncoder {
            fps: fps,
            enc_ctx: OpusEncoder::create_ctx(),
            frame_index: 0,
        }
    }

    fn create_ctx() -> av_codec::encoder::Context {
        // let audio_channel_map = av_data::audiosample::ChannelMap::default_map(2);
        let codec_info = av_data::params::AudioInfo {
            rate: 0,
            map: Some(OpusEncoder::channel_map_mono()),
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

        let encoder_list = av_codec::encoder::Codecs::from_list(&[OPUS_DESCR]);
        let mut enc_ctx = av_codec::encoder::Context::by_name(&encoder_list, &"opus").unwrap();
        enc_ctx.set_params(&codec_params).unwrap();
        enc_ctx.configure().unwrap();
        // enc_ctx.set_option("application", "audio").unwrap();

        enc_ctx
    }

    fn soniton() -> av_data::audiosample::Soniton {
        av_data::audiosample::Soniton {
            bits: 16,
            be: false,
            packed: false,
            planar: false,
            float: false,
            signed: true,
        }
    }

    fn channel_map_mono() -> av_data::audiosample::ChannelMap {
        av_data::audiosample::ChannelMap::default_map(1)
    }
}

impl Encoder for OpusEncoder {
    fn encode_video(&mut self, _frame: &VideoFrame) -> Result<EncodedFrame, String> {
        unimplemented!();
    }

    fn encode_audio(&mut self, frame: &AudioFrame) -> Result<EncodedFrame, String> {
        let now = utils::time::now_utc();
        let expired = now.sub(FRAME_EXPIRE_DURATION);
        if frame.timestamp < expired {
            self.frame_index += 1;

            return Err(format!("raw frame is decayed, dropping.. {:?} < {:?}", frame.timestamp, expired));
        }

        let av_frame = {
            let time_info = {
                let mut ti: av_data::timeinfo::TimeInfo = av_data::timeinfo::TimeInfo::default();
                ti.timebase = Some(av_data::rational::Rational64::new(1, self.fps as i64));
                ti.pts = Some(self.frame_index);
                ti
            };

            let frame_info = av_data::frame::AudioInfo {
                samples: frame.samples,
                rate: frame.sample_rate,
                map: OpusEncoder::channel_map_mono(),
                format: Arc::new(OpusEncoder::soniton()),
            };

            let frame_kind = av_data::frame::MediaKind::Audio(frame_info.clone());
            let arc_frame = {
                let mut f = av_data::frame::new_default_frame(frame_kind, Some(time_info));
                let mut buf = f.buf.as_mut_slice_inner(0).unwrap();
                utils::copy::copy_interleaved_sound_samples_mono(frame.buf.as_ref(), &mut buf);
                Arc::new(f)
            };
            arc_frame
        };

        let encoded_packet = {
            self.enc_ctx.send_frame(&av_frame).unwrap();
            self.enc_ctx.flush().unwrap();
            self.enc_ctx.receive_packet().unwrap()
        };

        self.frame_index += 1;

        Ok(EncodedFrame {
            buf: encoded_packet.data,
            timestamp: frame.timestamp,
        })
    }
}

pub struct H264Encoder {
    w: usize,
    h: usize,
    fps: usize,
    keyframe_interval: usize,

    enc_params: x264::Param,
    enc_ctx: x264::Encoder,

    frame_index: i64,
    encoded_frame_count: i64,
}

unsafe impl Send for H264Encoder {}

impl H264Encoder {
    pub fn create(w: usize, h: usize, fps: usize, keyframe_interval: usize) -> impl Encoder {
        let mut enc_params = H264Encoder::create_enc_params(w, h, keyframe_interval);
        let enc_ctx = x264::Encoder::open(&mut enc_params).unwrap();

        H264Encoder {
            w: w,
            h: h,
            fps: fps,
            keyframe_interval: keyframe_interval,
            enc_params: enc_params,
            enc_ctx: enc_ctx,
            frame_index: 0,
            encoded_frame_count: 0
        }
    }

    fn create_enc_params(w: usize, h: usize, kf_interval: usize) -> x264::Param {
        // https://obsproject.com/forum/resources/low-latency-high-performance-x264-options-for-for-most-streaming-services-youtube-facebook.726/
        x264::Param::default_preset("ultrafast", "zerolatency").unwrap()
            .set_dimension(h, w)
            .param_parse("sliced-threads", "1").unwrap()
            // .param_parse("interlaced", "1").unwrap()
            .param_parse("keyint", &kf_interval.to_string()).unwrap()
            .param_parse("min-keyint", &kf_interval.to_string()).unwrap()

            // - overriding on ultrafast preset
            // .param_parse("bframes", "2").unwrap()
            // .param_parse("b-adapt", "0").unwrap()
            // .param_parse("scenecut", "0").unwrap()
            // .param_parse("partitions", "none").unwrap()
            // .param_parse("no-weightb", "1").unwrap()
            // .param_parse("weightp", "0").unwrap()
            // .param_parse("sync-lookahead", "3").unwrap()
            // .param_parse("no-deblock", "1").unwrap()
            // .param_parse("aq-mode", "0").unwrap()
            // .param_parse("subme", "0").unwrap()
            // .param_parse("no-cabac", "1").unwrap()

            // - rate control option 1. 1 pass with crf
            .param_parse("pass", "1").unwrap()
            .param_parse("crf", "29").unwrap()

            // - rate control option 2. abr + vbv
            // .param_parse("pass", "2").unwrap()
            .param_parse("vbv-maxrate", "400").unwrap()
            .param_parse("vbv-bufsize", "400").unwrap()

            // - rate control option 3. cbr
            // .param_parse("nal-hrd", "cbr").unwrap()
            // .param_parse("bitrate", "400").unwrap()
            // .param_parse("force-cfr", "1").unwrap()
            // .param_parse("level", "3.0").unwrap()
            .apply_profile("baseline").unwrap()
    }
}

impl Encoder for H264Encoder {
    fn encode_audio(&mut self, _frame: &AudioFrame) -> Result<EncodedFrame, String> {
        unimplemented!();
    }

    fn encode_video(&mut self, frame: &VideoFrame) -> Result<EncodedFrame, String> {
        // skip encoding for expired frame
        let now = utils::time::now_utc();
        let expired = now.sub(FRAME_EXPIRE_DURATION);
        if frame.timestamp < expired {
            self.frame_index += 1;
            return Err(format!("raw frame is decayed, dropping.. {:?} < {:?}", frame.timestamp, expired));
        }

        let yuv_size = self.w * self.h;
        let chroma_size = yuv_size / 4;

        let mut y = vec![0u8; yuv_size];
        let mut u = vec![0u8; chroma_size];
        let mut v = vec![0u8; chroma_size];
        utils::converter::bgra_to_yuv420(self.w, self.h, &frame.buf, &mut y, &mut u, &mut v);

        let mut pic = x264::Picture::from_param(&self.enc_params).unwrap()
            .set_timestamp(self.frame_index);
        self.frame_index += 1;

        unsafe {
            ptr::copy(y.as_ptr(), pic.as_mut_slice(0).unwrap().as_mut_ptr(), yuv_size);
            ptr::copy(u.as_ptr(), pic.as_mut_slice(1).unwrap().as_mut_ptr(), chroma_size);
            ptr::copy(v.as_ptr(), pic.as_mut_slice(2).unwrap().as_mut_ptr(), chroma_size);
        };

        match self.enc_ctx.encode(&pic) {
            Ok(Some((nal, _, _))) => {
                let encoded = Vec::from(nal.as_bytes());

                self.encoded_frame_count += 1;

                Ok(EncodedFrame { buf: encoded, timestamp: frame.timestamp, })
            },

            Ok(None) => {
                Err(format!("nothing encoded.."))
            }

            Err(e) => {
                Err(format!("failed to encode frame.. {}", e))
            }
        }
    }
}
