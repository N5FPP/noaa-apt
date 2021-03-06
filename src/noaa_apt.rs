use wav;
use dsp;
use dsp::Signal;
use err;

use std;
use hound;
use png;

/// Resample wav file
///
/// The filter parameters are the default ones.
pub fn resample_wav(input_filename: &str, output_filename: &str,
                    output_rate: u32) -> err::Result<()> {

    info!("Reading WAV file");
    let (input_signal, input_spec) = wav::load_wav(input_filename)?;

    info!("Resampling");
    let resampled = dsp::resample_to(&input_signal, input_spec.sample_rate,
                                     output_rate);

    let writer_spec = hound::WavSpec {
        channels: 1,
        sample_rate: output_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    info!("Writing WAV to '{}'", output_filename);

    wav::write_wav(output_filename, &resampled, writer_spec)?;

   Ok(())
}

/// Decode APT image from WAV file.
pub fn decode(input_filename: &str, output_filename: &str) -> err::Result<()>{

    // Working sample rate, used during demodulation and syncing, better if
    // multiple of the final sample rate, 4160
    const WORK_RATE: u32 = 20800;

    // Final signal (with has one sample per pixel) sample rate
    const FINAL_RATE: u32 = 4160;

    // Pixels per row
    const PX_PER_ROW: u32 = 2080;


    info!("Reading WAV file");

    let (signal, input_spec) = wav::load_wav(input_filename)?;

    info!("Resampling to {}", WORK_RATE);

    let signal = dsp::resample_to(&signal, input_spec.sample_rate, WORK_RATE);

    info!("Demodulating");

    let atten = 40.;
    let delta_w = 1./20.;
    let signal = dsp::demodulate(&signal, atten, delta_w);

    info!("Syncing");

    let max: &f32 = dsp::get_max(&signal);

    // TODO define and resample to WORK_RATE
    // sync frame to find: seven impulses and some black pixels (some lines
    // have something like 8 black pixels and then white ones)
    let mut guard: Signal = Vec::with_capacity(20*7 + 35);
    for _i in 0..7 {
        guard.extend_from_slice(&[-1., -1., -1., -1., -1., -1., -1., -1., -1., -1.,
                                 1., 1., 1., 1., 1., 1., 1., 1., 1., 1.]);
    }
    for _i in 0..35 {
        guard.push(-1.);
    }

    // list of maximum correlations found: (index, value)
    let mut peaks: Vec<(usize, f32)> = Vec::new();
    peaks.push((0, 0.));

    // minimum distance between peaks, some arbitrary number smaller but close
    // to the number of samples by line
    let min_distance: usize = (PX_PER_ROW * WORK_RATE / FINAL_RATE) as usize * 8/10;

    // need to shift the values down to get meaningful correlation values
    for i in 0 .. signal.len() - guard.len() {
        let mut corr: f32 = 0.;
        for j in 0..guard.len() {
            corr += guard[j] * (signal[i+j] - *max/2.);
        }

        // if previous peak is too far, keep it and add this value to the
        // list as a new peak
        if i - peaks.last().unwrap().0 > min_distance {
            peaks.push((i, corr));
        }

        // else if this value is bigger than the previous maximum, set this
        // one
        else if corr > peaks.last().unwrap().1 {
            peaks.pop();
            peaks.push((i, corr));
        }
    }

    let mut aligned: Signal = Vec::new();

    for i in 0..peaks.len()-1 {
        aligned.extend_from_slice(&signal[peaks[i].0 ..
                peaks[i].0 + (PX_PER_ROW * WORK_RATE / FINAL_RATE) as usize]);
    }

    debug!("Resampling to 4160");

    let aligned = dsp::resample_to(&aligned, WORK_RATE, FINAL_RATE);
    let max = dsp::get_max(&aligned);

    debug!("Mapping samples from 0-{} to 0-255", max);

    // TODO simplify to one line
    let aligned: Signal = aligned.iter().map(|x| x/max).collect();
    let aligned: Vec<u8> = aligned.iter().map(|x| (x*255.) as u8).collect();

    info!("Writing PNG to '{}'", output_filename);

    // To use encoder.set()
    use png::HasParameters;

    let path = std::path::Path::new(output_filename);
    let file = std::fs::File::create(path)?;
    let ref mut buffer = std::io::BufWriter::new(file);

    let height = aligned.len() as u32 / PX_PER_ROW;

    let mut encoder = png::Encoder::new(buffer, PX_PER_ROW, height);
    encoder.set(png::ColorType::Grayscale).set(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;

    writer.write_image_data(&aligned[..])?;

    Ok(())
}
