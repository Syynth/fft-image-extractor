use clap::{arg, command, Parser};
use std::cmp::min;
use std::fs::File;

use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

use spectrum_analyzer::scaling::{scale_20_times_log10, scale_to_zero_to_one};
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};

use image::{ImageBuffer, Rgba};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct FftResampler {
    /// Input audio file
    #[arg(short, long)]
    file: String,

    /// Width of the output image
    /// Controls the number of frequency bins
    #[arg(short, long, default_value = "32")]
    width: u32,

    /// Speed of the input audio
    #[arg(short, long, default_value = "160")]
    bpm: f32,
}

fn extract_samples(file: Box<File>) -> Vec<f32> {
    let mss = MediaSourceStream::new(file, Default::default());

    let hint = Hint::new();
    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();
    let decoder_opts: DecoderOptions = Default::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .unwrap();

    let mut format = probed.format;

    let track = format.default_track().unwrap();

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .unwrap();

    let track_id = track.id;

    let mut sample_count = 0;
    let mut sample_buf = None;
    let mut all_samples = Vec::new();

    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if sample_buf.is_none() {
                    let spec = *audio_buf.spec();

                    let duration = audio_buf.capacity() as u64;

                    sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                }

                if let Some(buf) = &mut sample_buf {
                    buf.copy_interleaved_ref(audio_buf);

                    sample_count += buf.samples().len();
                    all_samples.extend_from_slice(buf.samples());
                    print!("\rDecoded {} samples", sample_count);
                }
            }
            Err(Error::DecodeError(_)) => (),
            Err(_) => break,
        }
    }
    return all_samples;
}

const SAMPLING_RATE: u32 = 44_100;
const SAMPLING_WINDOW: usize = 2048;

fn main() {
    let cli = FftResampler::parse();

    let file = Box::new(File::open(&cli.file).unwrap());
    let audio_samples = extract_samples(file);
    let sample_count = audio_samples.len();

    println!("\nFinished, with {} samples", audio_samples.len());
    let w = sample_count / SAMPLING_WINDOW;
    let h = cli.width;

    let mut img = ImageBuffer::new(w as u32, h / 8);

    for x in 0..w {
        print!("Processing column {} of {}\r", x, w);
        let sample_end = min((x + 1) * SAMPLING_WINDOW, sample_count);
        let sample_start = x * SAMPLING_WINDOW;
        let freqs = samples_fft_to_spectrum(
            &audio_samples[sample_start..sample_end],
            SAMPLING_RATE,
            FrequencyLimit::Range(0.0, 10_000.0),
            Some(&scale_to_zero_to_one),
        )
        .unwrap();

        let data = freqs.data();
        let data_size = data.len();

        for y in (0..min(h as usize / 2, data_size)).step_by(4) {
            let mut pixel = [0u8; 4]; // Initialize pixel with 4 sub-pixel components
            for i in 0..4usize {
                if y + i < data_size {
                    let val = freqs.data()[y + i].1.val();
                    pixel[i] = (val * 255.0) as u8;
                }
            }
            img.put_pixel(x as u32, (y as u32) / 4, Rgba(pixel));
        }
    }

    println!("\nSaving image...");
    img.save(format!("{}.png", cli.file)).unwrap();
}
