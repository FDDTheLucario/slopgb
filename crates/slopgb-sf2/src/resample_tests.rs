use super::*;

#[test]
fn identity_at_1_to_1_ratio() {
    let pcm = vec![0i16, 100, -100, 32767, -32768, 42];
    assert_eq!(resample(&pcm, 32000, 32000), pcm);
}

#[test]
fn empty_input_is_empty_output() {
    assert!(resample(&[], 32000, 44100).is_empty());
}

/// Up-sample then down-sample a pure sine and check it is still recognizably
/// the same sine: Pearson correlation with the original above 0.95 (a broken
/// resampler — e.g. reversed, garbled, or silent — would score far lower).
#[test]
fn sine_stays_recognizable_after_up_and_down_resample() {
    const N: usize = 800;
    let original: Vec<i16> = (0..N)
        .map(|i| ((i as f64 * 0.05).sin() * 10000.0) as i16)
        .collect();

    let up = resample(&original, 32000, 48000);
    let back = resample(&up, 48000, 32000);
    assert_eq!(back.len(), original.len());

    let n = original.len() as f64;
    let mean_a: f64 = original.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let mean_b: f64 = back.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for (&a, &b) in original.iter().zip(&back) {
        let da = f64::from(a) - mean_a;
        let db = f64::from(b) - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    let correlation = cov / (var_a.sqrt() * var_b.sqrt());
    assert!(
        correlation > 0.95,
        "correlation {correlation} did not exceed 0.95 bound"
    );
}
