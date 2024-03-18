use clap::{arg, command, Parser};
use std::cmp::min;
use std::fs::File;

use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

use image::{ImageBuffer, Rgba};
use spectrum_analyzer::scaling::scale_to_zero_to_one;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use std::f32::consts::LN_10;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct FftResampler {
    /// Input audio file
    #[arg(short, long)]
    file: String,

    /// Width of the output image
    /// Controls the number of frequency bins
    #[arg(short, long, default_value = "128")]
    width: u32,
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
    all_samples
}

const SAMPLING_RATE: u32 = 44_100;
const SAMPLING_WINDOW: usize = 2048;
const FREQUENCY_MAX: f32 = 10_000.0;

fn nearest_power_of_two_below(x: u32) -> u32 {
    let mut n = 1;
    while n * 2 < x {
        n *= 2;
    }
    n
}

fn nearest_power_of_two_above(x: u32) -> u32 {
    let mut n = 1;
    while n < x {
        n *= 2;
    }
    n
}

fn main() {
    let cli = FftResampler::parse();

    let file = Box::new(File::open(&cli.file).unwrap());
    let audio_samples = extract_samples(file);
    let sample_count = audio_samples.len();

    println!("\nFinished, with {} samples", audio_samples.len());
    let total_width = sample_count / SAMPLING_WINDOW;
    let h = cli.width;

    // Find the nearest power of two to the total width
    let nearest_w = nearest_power_of_two_below(total_width as u32);
    let w: usize = nearest_w as usize / 4; // width of a single row

    let img_row_height = h;
    let row_count: u32 = (total_width / w) as u32 + 1;

    let img_height = img_row_height * row_count;

    let mut img = ImageBuffer::new(w as u32, img_height);

    let freq_min: f32 = 20.0; // Minimum frequency (Hz)

    let log_freq_min = freq_min.ln() / LN_10;
    let log_freq_max = FREQUENCY_MAX.ln() / LN_10;

    for sampling_x in 0..total_width {
        print!("Processing column {} of {}\r", sampling_x, total_width);
        let sample_end = min((sampling_x + 1) * SAMPLING_WINDOW, sample_count);
        let sample_start = sampling_x * SAMPLING_WINDOW;
        let freqs = samples_fft_to_spectrum(
            &audio_samples[sample_start..sample_end],
            SAMPLING_RATE,
            FrequencyLimit::Range(0.0, FREQUENCY_MAX),
            Some(&scale_to_zero_to_one),
        )
        .unwrap();

        let data = freqs.data();
        let data_size = data.len();

        let mut prev_val: u8 = 0;

        let img_x = (sampling_x % w) as u32;
        let img_row_offset = (sampling_x / w) as u32 * img_row_height;
        let mut img_row = img_row_offset;

        for sampling_y in 0..data_size {
            let freq = freqs.data()[sampling_y].0.val();
            let log_freq = freq.ln() / LN_10;

            if log_freq < log_freq_min || log_freq > log_freq_max {
                continue;
            }

            let img_row_target: u32 = ((log_freq - log_freq_min) / (log_freq_max - log_freq_min)
                * img_row_height as f32)
                .round() as u32
                + img_row_offset;

            while img_row < img_row_target {
                let pixel = [prev_val; 4];
                img.put_pixel(img_x, img_row, Rgba(pixel));
                img_row += 1;
            }

            let val = freqs.data()[sampling_y].1.val();
            prev_val = (val * 255.0) as u8;
        }

        while img_row < img_height {
            let pixel = [prev_val; 4];
            img.put_pixel(img_x, img_row, Rgba(pixel));
            img_row += 1;
        }
    }

    let img_name = format!("{}.png", cli.file);
    println!("\nSaving image as {img_name:?} ...");
    img.save(img_name).unwrap();
}
