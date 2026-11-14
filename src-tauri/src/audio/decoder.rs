//! FFmpeg-backed decoder. Decodes any format FFmpeg understands (FLAC, ALAC, APE,
//! WavPack, DSD, WMA Lossless, Opus, ...) into interleaved f32 at the output rate.

use anyhow::{anyhow, Context as _, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg::format::{sample::Type as SampleType, Sample};
use ffmpeg::software::resampling;
use ffmpeg::util::frame::Audio as AudioFrame;
use ffmpeg::ChannelLayout;
use serde::Serialize;

const LOSSLESS_CODECS: &[&str] = &[
    "flac", "alac", "ape", "wavpack", "tta", "wmalossless", "truehd", "mlp", "shorten", "tak",
];

pub fn is_lossless_codec(name: &str) -> bool {
    name.starts_with("pcm_") || name.starts_with("dsd_") || LOSSLESS_CODECS.contains(&name)
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StreamInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub channels: u16,
    pub lossless: bool,
    pub duration: f64,
    pub output_rate: u32,
}

pub fn init() {
    let _ = ffmpeg::init();
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Error);
}

pub struct Decoder {
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Audio,
    stream_index: usize,
    time_base: f64,
    resampler: Option<(resampling::Context, Sample, u64, u32)>,
    out_rate: u32,
    out_layout: ChannelLayout,
    out_channels: usize,
    eof_sent: bool,
    finished: bool,
    /// After a seek, drop decoded audio until this timestamp (seconds).
    skip_until: Option<f64>,
    pub info: StreamInfo,
}

impl Decoder {
    pub fn open(source: &str, out_rate: u32, out_channels: u16) -> Result<Self> {
        let input = ffmpeg::format::input(&source).with_context(|| format!("cannot open {source}"))?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .ok_or_else(|| anyhow!("no audio stream"))?;
        let stream_index = stream.index();
        let tb = stream.time_base();
        let time_base = tb.numerator() as f64 / tb.denominator().max(1) as f64;
        let ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let codec = ctx.id().name().to_string();
        let decoder = ctx.decoder().audio().context("unsupported codec")?;

        let bits_raw = unsafe { (*decoder.as_ptr()).bits_per_raw_sample } as u32;
        let bit_depth = if bits_raw > 0 {
            bits_raw
        } else if codec.starts_with("dsd_") {
            1
        } else {
            decoder.format().bytes() as u32 * 8
        };
        let duration = if input.duration() > 0 {
            input.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64
        } else {
            0.0
        };
        let info = StreamInfo {
            lossless: is_lossless_codec(&codec),
            codec,
            sample_rate: decoder.rate(),
            bit_depth,
            channels: decoder.channels(),
            duration,
            output_rate: out_rate,
        };

        Ok(Self {
            input,
            decoder,
            stream_index,
            time_base,
            resampler: None,
            out_rate,
            out_layout: ChannelLayout::default(out_channels as i32),
            out_channels: out_channels as usize,
            eof_sent: false,
            finished: false,
            skip_until: None,
            info,
        })
    }

    pub fn seek(&mut self, secs: f64) -> Result<()> {
        let ts = (secs.max(0.0) * ffmpeg::ffi::AV_TIME_BASE as f64) as i64;
        self.input.seek(ts, ..ts)?;
        self.decoder.flush();
        self.resampler = None;
        self.eof_sent = false;
        self.finished = false;
        self.skip_until = Some(secs);
        Ok(())
    }

    /// Appends the next chunk of interleaved samples to `out`. Returns false at end of stream.
    pub fn decode_next(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        if self.finished {
            return Ok(false);
        }
        let mut frame = AudioFrame::empty();
        loop {
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => {
                    let start = out.len();
                    self.resample(&frame, out)?;
                    self.apply_seek_trim(&frame, out, start);
                    return Ok(true);
                }
                Err(ffmpeg::Error::Eof) => {
                    self.finished = true;
                    return Ok(false);
                }
                Err(_) if self.eof_sent => {
                    self.finished = true;
                    return Ok(false);
                }
                Err(_) => {}
            }
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) => {
                    if packet.stream() == self.stream_index {
                        // Corrupt packets are skipped rather than aborting playback.
                        let _ = self.decoder.send_packet(&packet);
                    }
                }
                Err(ffmpeg::Error::Eof) | Err(_) => {
                    let _ = self.decoder.send_eof();
                    self.eof_sent = true;
                }
            }
        }
    }

    fn resample(&mut self, frame: &AudioFrame, out: &mut Vec<f32>) -> Result<()> {
        let mut layout = frame.channel_layout();
        if layout.is_empty() || layout.channels() != frame.channels() as i32 {
            layout = ChannelLayout::default(frame.channels() as i32);
        }
        let key = (frame.format(), layout.bits(), frame.rate());
        let stale = match &self.resampler {
            Some((_, f, l, r)) => (*f, *l, *r) != key,
            None => true,
        };
        if stale {
            let ctx = resampling::Context::get(
                frame.format(),
                layout,
                frame.rate(),
                Sample::F32(SampleType::Packed),
                self.out_layout,
                self.out_rate,
            )?;
            self.resampler = Some((ctx, key.0, key.1, key.2));
        }
        let (ctx, ..) = self.resampler.as_mut().unwrap();

        let mut input = frame.clone();
        input.set_channel_layout(layout);
        let capacity = frame.samples() * self.out_rate as usize / frame.rate().max(1) as usize + 512;
        let mut converted = AudioFrame::new(Sample::F32(SampleType::Packed), capacity, self.out_layout);
        ctx.run(&input, &mut converted)?;
        let n = converted.samples() * self.out_channels;
        let bytes = &converted.data(0)[..n * 4];
        out.extend(bytes.chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])));
        Ok(())
    }

    fn apply_seek_trim(&mut self, frame: &AudioFrame, out: &mut Vec<f32>, start: usize) {
        let Some(target) = self.skip_until else { return };
        let Some(pts) = frame.pts() else {
            self.skip_until = None;
            return;
        };
        let t = pts as f64 * self.time_base;
        let produced_frames = (out.len() - start) / self.out_channels;
        let drop_frames = ((target - t) * self.out_rate as f64).round().max(0.0) as usize;
        if drop_frames >= produced_frames {
            out.truncate(start);
        } else {
            out.drain(start..start + drop_frames * self.out_channels);
            self.skip_until = None;
        }
    }
}
