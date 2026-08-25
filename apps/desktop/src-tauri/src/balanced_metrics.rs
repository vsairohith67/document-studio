use crate::contracts::OperationError;
use crate::contracts::OperationStage;
use std::collections::VecDeque;

pub const BALANCED_SSIM_MINIMUM: f64 = 0.985;
pub const BALANCED_PSNR_MINIMUM_DB: f64 = 36.0;
pub const BALANCED_CHANGED_DELTA_THRESHOLD: u8 = 12;

const WINDOW_SIZE: usize = 11;
const WINDOW_RADIUS: usize = WINDOW_SIZE / 2;
const GAUSSIAN_SIGMA: f64 = 1.5;
const C1: f64 = 6.5025;
const C2: f64 = 58.5225;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageQualityMetrics {
    pub ssim: f64,
    pub psnr_db: Option<f64>,
    pub changed_pixels: u64,
    pub total_pixels: u64,
}

impl PageQualityMetrics {
    pub fn passes(self) -> bool {
        self.ssim >= BALANCED_SSIM_MINIMUM
            && self
                .psnr_db
                .is_none_or(|value| value >= BALANCED_PSNR_MINIMUM_DB)
            && changed_ratio_passes(self.changed_pixels, self.total_pixels)
    }
}

#[derive(Default)]
struct CompensatedSum {
    sum: f64,
    compensation: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct WindowMoments {
    source: f64,
    candidate: f64,
    source_square: f64,
    candidate_square: f64,
    product: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let corrected = value - self.compensation;
        let next = self.sum + corrected;
        self.compensation = (next - self.sum) - corrected;
        self.sum = next;
    }

    fn value(&self) -> f64 {
        self.sum
    }
}

pub fn compare_rgb8(
    source: &[u8],
    candidate: &[u8],
    width: u32,
    height: u32,
) -> Result<PageQualityMetrics, OperationError> {
    if width < WINDOW_SIZE as u32 || height < WINDOW_SIZE as u32 {
        return Err(metric_input_error());
    }
    let total_pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(metric_input_error)?;
    let expected = total_pixels
        .checked_mul(3)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(metric_input_error)?;
    if source.len() != expected || candidate.len() != expected {
        return Err(metric_input_error());
    }

    let mut square_error = 0_u128;
    let mut changed_pixels = 0_u64;
    for (left, right) in source.chunks_exact(3).zip(candidate.chunks_exact(3)) {
        let mut maximum_delta = 0_u8;
        for channel in 0..3 {
            let delta = left[channel].abs_diff(right[channel]);
            maximum_delta = maximum_delta.max(delta);
            square_error = square_error
                .checked_add(u128::from(delta) * u128::from(delta))
                .ok_or_else(metric_input_error)?;
        }
        if maximum_delta > BALANCED_CHANGED_DELTA_THRESHOLD {
            changed_pixels = changed_pixels
                .checked_add(1)
                .ok_or_else(metric_input_error)?;
        }
    }

    let psnr_db = if square_error == 0 {
        None
    } else {
        let denominator = total_pixels.checked_mul(3).ok_or_else(metric_input_error)? as f64;
        let mean_square_error = square_error as f64 / denominator;
        Some(10.0 * (255.0_f64 * 255.0 / mean_square_error).log10())
    };

    let weights = gaussian_weights();
    let width = usize::try_from(width).map_err(|_| metric_input_error())?;
    let height = usize::try_from(height).map_err(|_| metric_input_error())?;
    let mut ssim_sum = CompensatedSum::default();
    let mut windows = 0_u64;
    let valid_width = width - WINDOW_SIZE + 1;
    let mut horizontal_rows = VecDeque::<Vec<WindowMoments>>::with_capacity(WINDOW_SIZE);
    for pixel_y in 0..height {
        let mut horizontal = Vec::new();
        horizontal
            .try_reserve_exact(valid_width)
            .map_err(|_| metric_input_error())?;
        for left_x in 0..valid_width {
            let mut source_mean = CompensatedSum::default();
            let mut candidate_mean = CompensatedSum::default();
            let mut source_square = CompensatedSum::default();
            let mut candidate_square = CompensatedSum::default();
            let mut product = CompensatedSum::default();
            for (window_x, weight) in weights.iter().copied().enumerate() {
                let pixel_x = left_x + window_x;
                let offset = (pixel_y * width + pixel_x) * 3;
                let left = nonlinear_bt709_luma(&source[offset..offset + 3]);
                let right = nonlinear_bt709_luma(&candidate[offset..offset + 3]);
                source_mean.add(weight * left);
                candidate_mean.add(weight * right);
                source_square.add(weight * left * left);
                candidate_square.add(weight * right * right);
                product.add(weight * left * right);
            }
            horizontal.push(WindowMoments {
                source: source_mean.value(),
                candidate: candidate_mean.value(),
                source_square: source_square.value(),
                candidate_square: candidate_square.value(),
                product: product.value(),
            });
        }
        horizontal_rows.push_back(horizontal);
        if horizontal_rows.len() < WINDOW_SIZE {
            continue;
        }
        for horizontal_x in 0..valid_width {
            let mut source_mean = CompensatedSum::default();
            let mut candidate_mean = CompensatedSum::default();
            let mut source_square = CompensatedSum::default();
            let mut candidate_square = CompensatedSum::default();
            let mut product = CompensatedSum::default();
            for (window_y, row) in horizontal_rows.iter().enumerate() {
                let weight = weights[window_y];
                let moments = row[horizontal_x];
                source_mean.add(weight * moments.source);
                candidate_mean.add(weight * moments.candidate);
                source_square.add(weight * moments.source_square);
                candidate_square.add(weight * moments.candidate_square);
                product.add(weight * moments.product);
            }
            let source_mean = source_mean.value();
            let candidate_mean = candidate_mean.value();
            let source_variance = (source_square.value() - source_mean * source_mean).max(0.0);
            let candidate_variance =
                (candidate_square.value() - candidate_mean * candidate_mean).max(0.0);
            let covariance = product.value() - source_mean * candidate_mean;
            let numerator = (2.0 * source_mean * candidate_mean + C1) * (2.0 * covariance + C2);
            let denominator = (source_mean * source_mean + candidate_mean * candidate_mean + C1)
                * (source_variance + candidate_variance + C2);
            if !numerator.is_finite() || !denominator.is_finite() || denominator <= 0.0 {
                return Err(metric_input_error());
            }
            ssim_sum.add(numerator / denominator);
            windows = windows.checked_add(1).ok_or_else(metric_input_error)?;
        }
        horizontal_rows.pop_front();
    }
    if windows == 0 {
        return Err(metric_input_error());
    }
    let ssim = ssim_sum.value() / windows as f64;
    if !ssim.is_finite() || psnr_db.is_some_and(|value| !value.is_finite()) {
        return Err(metric_input_error());
    }
    Ok(PageQualityMetrics {
        ssim,
        psnr_db,
        changed_pixels,
        total_pixels,
    })
}

pub fn changed_ratio_passes(changed_pixels: u64, total_pixels: u64) -> bool {
    total_pixels > 0 && u128::from(changed_pixels) * 200_u128 <= u128::from(total_pixels)
}

fn nonlinear_bt709_luma(pixel: &[u8]) -> f64 {
    0.2126 * f64::from(pixel[0]) + 0.7152 * f64::from(pixel[1]) + 0.0722 * f64::from(pixel[2])
}

fn gaussian_weights() -> [f64; WINDOW_SIZE] {
    let mut one_dimensional = [0.0_f64; WINDOW_SIZE];
    let mut normalization = CompensatedSum::default();
    for (index, value) in one_dimensional.iter_mut().enumerate() {
        let offset = index as f64 - WINDOW_RADIUS as f64;
        *value = (-(offset * offset) / (2.0 * GAUSSIAN_SIGMA * GAUSSIAN_SIGMA)).exp();
        normalization.add(*value);
    }
    let normalization = normalization.value();
    for value in &mut one_dimensional {
        *value /= normalization;
    }
    one_dimensional
}

fn metric_input_error() -> OperationError {
    OperationError::safe(
        "BALANCED_METRIC_INPUT_INVALID",
        "The visual comparison data is invalid",
        "No output was published because the bounded page comparison could not be verified.",
        OperationStage::Verify,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, value: u8) -> Vec<u8> {
        vec![value; width * height * 3]
    }

    #[test]
    fn identical_vector_has_exact_identity_metrics() {
        let pixels = solid(16, 16, 100);
        let metrics = compare_rgb8(&pixels, &pixels, 16, 16).unwrap();
        assert_eq!(metrics.ssim, 1.0);
        assert_eq!(metrics.psnr_db, None);
        assert_eq!(metrics.changed_pixels, 0);
        assert!(metrics.passes());
    }

    #[test]
    fn uniform_one_step_vector_matches_known_ssim_and_psnr() {
        let source = solid(16, 16, 100);
        let candidate = solid(16, 16, 101);
        let metrics = compare_rgb8(&source, &candidate, 16, 16).unwrap();
        let expected_ssim =
            (2.0 * 100.0 * 101.0 + C1) / (100.0_f64.powi(2) + 101.0_f64.powi(2) + C1);
        let expected_psnr = 20.0 * 255.0_f64.log10();
        assert!((metrics.ssim - expected_ssim).abs() < 1e-12);
        assert!((metrics.psnr_db.unwrap() - expected_psnr).abs() < 1e-12);
        assert!(metrics.passes());
    }

    #[test]
    fn changed_pixel_threshold_is_exactly_greater_than_twelve() {
        let source = solid(20, 20, 0);
        let mut candidate = source.clone();
        candidate[0] = 12;
        let at_twelve = compare_rgb8(&source, &candidate, 20, 20).unwrap();
        assert_eq!(at_twelve.changed_pixels, 0);
        candidate[0] = 13;
        let at_thirteen = compare_rgb8(&source, &candidate, 20, 20).unwrap();
        assert_eq!(at_thirteen.changed_pixels, 1);
    }

    #[test]
    fn changed_ratio_boundary_is_exactly_half_a_percent() {
        assert!(changed_ratio_passes(2, 400));
        assert!(!changed_ratio_passes(3, 400));
    }

    #[test]
    fn wrong_dimensions_fail_closed() {
        let pixels = solid(16, 16, 0);
        assert!(compare_rgb8(&pixels, &pixels, 15, 16).is_err());
        assert!(compare_rgb8(&pixels, &pixels, 10, 16).is_err());
    }
}
