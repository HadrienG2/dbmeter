use atomic::{Atomic, Ordering};
use crate::{Decibel, Sample};
use std::panic::{RefUnwindSafe, UnwindSafe};


// A basic peak meter meant for interactive displays
//
// Uses the highest sample in the audio data as the peak value. Beware, this
// method underestimates the true peak. Badness of underestimation varies from
// 3 dB at a quarter of the Nyquist frequency, to unbounded amounts at half of
// the Nyquist frequency, which is bad because a significant fraction of
// transient energy lies at high frequencies.
//
// The fix is to use a True Peak meter, which does 4x oversampling with lowpass
// filtering before looking at the peak sample of that signal. I'll implement
// one later, and then this one will forever become a demo toy.
//
pub struct SamplePeakMeter {
    // Current peak value, as an FP sample
    peak_sample: Atomic<Sample>,
}

impl SamplePeakMeter {
    // Create a sample-based peak-meter
    pub fn new() -> Self {
        Self { peak_sample: Atomic::new(0.0) }
    }

    // Feed new data into the peak meter
    pub fn integrate(&self, data: impl IntoIterator<Item=Sample>) {
        let max = data.into_iter()
                      .map(|x| x.abs())
                      .fold(0.0f32, |x, y| x.max(y));
        let mut old_max = self.peak_sample.load(Ordering::Relaxed);
        while max > old_max {
            match self.peak_sample.compare_exchange(old_max,
                                                    max,
                                                    Ordering::Relaxed,
                                                    Ordering::Relaxed) {
                Ok(_) => return,
                Err(new_old_max) => old_max = new_old_max,
            }
        }
    }


    // Query the current value of the peak meter in dBFS and reset it to zero
    pub fn read_and_reset(&self) -> Decibel {
        20.0 * self.peak_sample.swap(0., Ordering::Relaxed).log10()
    }
}

// FIXME: Atomic crate should do this for me
impl UnwindSafe for SamplePeakMeter {}
impl RefUnwindSafe for SamplePeakMeter {}


// A basic VU-meter-ish thing
//
// It does not actually measure VU, being dBFS-based, but that doesn't actually
// matter because you shouldn't use a VU-meter anyway. These things are a poor
// measure of loudness as they don't account for the frequency response of
// human hearing.
//
// As before, I'll write an LUFS meter someday, then this will become a mere
// example program that one shouldn't actually use in production.
//
pub struct VUMeter {
    // Current VU value, as an FP sample
    vu_sample: Atomic<Sample>,

    // Weight of old VU vs new samples
    vu_weight: Atomic<f32>,
}

impl VUMeter {
    // Set up a VU-meter for a given sampling rate
    pub fn new(sampling_rate: u32) -> Self {
        Self {
            vu_sample: Atomic::new(0.0),
            vu_weight: Atomic::new(Self::vu_weight(sampling_rate)),
        }
    }

    // Compute the VU weight for a given sampling rate
    fn vu_weight(sampling_rate: u32) -> f32 {
        // So, we have sample ~ a x sin(... x t), for every new sample we do...
        //
        //     corr_spl = corr x |sample|
        //     VU_new = corr_spl + (VU_old - corr_spl) * exp(-t/tau)
        //
        // ...and we want to pick tau and corr such that
        //
        //     VU_new(300ms) = 0.99 * |a|
        //
        // The average of the absolute value of a sine is its amplitude times
        // 2/pi, so corr = pi/2. And ln(0.01) ~ 4.6 so we want
        // 300ms = 4.6 * tau => tau = 300ms / 4.6.
        //
        // But wait. dt between two samples is fully determined by the sampling
        // rate. So for a given sampling rate, we can hardcode the value of this
        // exponential as "weight" and do just on every sample...
        //
        //     VU_new = corr_spl + (VU_old - corr_spl) * weight
        //
        // That's awesome! Let's do it then.
        //
        const RISE_TIME: f32 = 0.3;
        const RISE_PRECISION: f32 = 0.01;
        let tau = -RISE_TIME / RISE_PRECISION.ln();
        let dt = 1.0 / (sampling_rate as f32);
        (-dt/tau).exp()
    }

    // Update the sampling rate, please remember to call this if your audio
    // API allows changing the sampling rate in the middle of an audio stream.
    pub fn update_sampling_rate(&self, sampling_rate: u32) {
        self.vu_weight.store(Self::vu_weight(sampling_rate), Ordering::Relaxed);
    }

    // Feed samples into the API
    pub fn integrate<I, II>(&self, data: II)
        where II: IntoIterator<Item=Sample, IntoIter=I>,
              I: Iterator<Item=Sample> + Clone,
    {
        const AMPLITUDE_CORRECTION: f32 = std::f32::consts::PI / 2.0;
        let data_iter = data.into_iter();
        let mut old_vu = self.vu_sample.load(Ordering::Relaxed);
        loop {
            let vu_weight =
                self.vu_weight.load(Ordering::Relaxed);
            let new_vu =
                data_iter.clone()
                         .map(|spl| spl.abs() * AMPLITUDE_CORRECTION)
                         .fold(old_vu, |vu, spl| {
                             spl + (vu - spl) * vu_weight
                         });
            match self.vu_sample.compare_exchange(old_vu,
                                                  new_vu,
                                                  Ordering::Relaxed,
                                                  Ordering::Relaxed) {
                Ok(_) => return,
                Err(new_old_vu) => old_vu = new_old_vu,
            }
        }
    }

    // Read the current VU-meter value in VUFS
    pub fn read(&self) -> Decibel {
        20.0 * self.vu_sample.load(Ordering::Relaxed).log10()
    }
}

// FIXME: Atomic crate should do this for me
impl UnwindSafe for VUMeter {}
impl RefUnwindSafe for VUMeter {}