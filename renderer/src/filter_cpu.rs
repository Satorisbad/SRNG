use crate::filter::*;
use crate::Rgba;
use std::collections::HashMap;
use vello_cpu::Pixmap;

pub fn execute_filter_graph(
    source: &Pixmap,
    graph: &FilterGraph,
    width: u16,
    height: u16,
) -> Result<Pixmap, String> {
    let mut results: HashMap<String, Pixmap> = HashMap::new();

    for node in &graph.nodes {
        let output = match &node.op {
            FilterPrimitive::GaussianBlur {
                input,
                sigma_x,
                sigma_y,
            } => {
                let mut pixmap = resolve(input, source, &results, width, height)?;
                gaussian_blur(&mut pixmap, *sigma_x, *sigma_y, width, height);
                pixmap
            }
            FilterPrimitive::Offset { input, dx, dy } => {
                let mut pixmap = resolve(input, source, &results, width, height)?;
                offset_pixmap(
                    &mut pixmap,
                    dx.round() as i32,
                    dy.round() as i32,
                    width,
                    height,
                );
                pixmap
            }
            FilterPrimitive::Blend {
                input,
                input2,
                mode,
            } => {
                let first = resolve(input, source, &results, width, height)?;
                let second = resolve(input2, source, &results, width, height)?;
                blend(&first, &second, *mode, width, height)
            }
            FilterPrimitive::Composite {
                input,
                input2,
                operator,
            } => {
                let first = resolve(input, source, &results, width, height)?;
                let second = resolve(input2, source, &results, width, height)?;
                composite(&first, &second, *operator, width, height)
            }
            FilterPrimitive::ColorMatrix { input, matrix } => {
                let mut pixmap = resolve(input, source, &results, width, height)?;
                color_matrix(&mut pixmap, matrix);
                pixmap
            }
            FilterPrimitive::Flood { color } => flood(*color, width, height),
            FilterPrimitive::Merge { inputs } => {
                merge(inputs, source, &results, width, height)?
            }
            FilterPrimitive::Morphology {
                input,
                operator,
                radius_x,
                radius_y,
            } => {
                let pixmap = resolve(input, source, &results, width, height)?;
                morphology(
                    &pixmap,
                    *operator,
                    radius_x.round() as i32,
                    radius_y.round() as i32,
                    width,
                    height,
                )
            }
            FilterPrimitive::ComponentTransfer {
                input,
                red,
                green,
                blue,
                alpha,
            } => {
                let mut pixmap = resolve(input, source, &results, width, height)?;
                component_transfer(&mut pixmap, red, green, blue, alpha);
                pixmap
            }
            FilterPrimitive::Unsupported { input, .. } => {
                resolve(input, source, &results, width, height)?
            }
        };

        results.insert(node.result.clone(), output);
    }

    let mut output = resolve(&graph.output, source, &results, width, height)?;
    clip_region(&mut output, graph.region, width, height);
    Ok(output)
}

fn resolve(
    input: &FilterInput,
    source: &Pixmap,
    results: &HashMap<String, Pixmap>,
    width: u16,
    height: u16,
) -> Result<Pixmap, String> {
    match input {
        FilterInput::SourceGraphic => Ok(copy_pixmap(source, width, height)),
        FilterInput::SourceAlpha => Ok(source_alpha(source, width, height)),
        FilterInput::Named(name) => results
            .get(name)
            .map(|pixmap| copy_pixmap(pixmap, width, height))
            .ok_or_else(|| format!("filter input result `{name}` does not exist")),
    }
}

fn copy_pixmap(source: &Pixmap, width: u16, height: u16) -> Pixmap {
    let mut output = Pixmap::new(width, height);
    output
        .data_as_u8_slice_mut()
        .copy_from_slice(source.data_as_u8_slice());
    output.recompute_may_have_transparency();
    output
}

fn source_alpha(source: &Pixmap, width: u16, height: u16) -> Pixmap {
    let mut output = Pixmap::new(width, height);
    for (destination, source_pixel) in output
        .data_as_u8_slice_mut()
        .chunks_exact_mut(4)
        .zip(source.data_as_u8_slice().chunks_exact(4))
    {
        destination[0] = 0;
        destination[1] = 0;
        destination[2] = 0;
        destination[3] = source_pixel[3];
    }
    output.recompute_may_have_transparency();
    output
}

fn flood(color: Rgba, width: u16, height: u16) -> Pixmap {
    let mut output = Pixmap::new(width, height);
    for pixel in output.data_as_u8_slice_mut().chunks_exact_mut(4) {
        pixel.copy_from_slice(&[color.r, color.g, color.b, color.a]);
    }
    output.recompute_may_have_transparency();
    output
}

fn blend(
    first: &Pixmap,
    second: &Pixmap,
    mode: BlendMode,
    width: u16,
    height: u16,
) -> Pixmap {
    let mut output = Pixmap::new(width, height);

    for ((destination, first_pixel), second_pixel) in output
        .data_as_u8_slice_mut()
        .chunks_exact_mut(4)
        .zip(first.data_as_u8_slice().chunks_exact(4))
        .zip(second.data_as_u8_slice().chunks_exact(4))
    {
        let first_alpha = f64::from(first_pixel[3]) / 255.0;
        let second_alpha = f64::from(second_pixel[3]) / 255.0;
        let output_alpha = first_alpha + second_alpha - first_alpha * second_alpha;

        for channel in 0..3 {
            let first_value = f64::from(first_pixel[channel]) / 255.0;
            let second_value = f64::from(second_pixel[channel]) / 255.0;
            let blended = match mode {
                BlendMode::Normal => first_value,
                BlendMode::Multiply => first_value * second_value,
                BlendMode::Screen => 1.0 - (1.0 - first_value) * (1.0 - second_value),
                BlendMode::Darken => first_value.min(second_value),
                BlendMode::Lighten => first_value.max(second_value),
            };
            let premultiplied = (1.0 - second_alpha) * first_value * first_alpha
                + (1.0 - first_alpha) * second_value * second_alpha
                + first_alpha * second_alpha * blended;

            destination[channel] = if output_alpha > 0.0 {
                (premultiplied / output_alpha * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8
            } else {
                0
            };
        }
        destination[3] = (output_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    }

    output.recompute_may_have_transparency();
    output
}

fn composite(
    first: &Pixmap,
    second: &Pixmap,
    operator: CompositeOperator,
    width: u16,
    height: u16,
) -> Pixmap {
    let mut output = Pixmap::new(width, height);

    for ((destination, first_pixel), second_pixel) in output
        .data_as_u8_slice_mut()
        .chunks_exact_mut(4)
        .zip(first.data_as_u8_slice().chunks_exact(4))
        .zip(second.data_as_u8_slice().chunks_exact(4))
    {
        match operator {
            CompositeOperator::Arithmetic { k1, k2, k3, k4 } => {
                for channel in 0..4 {
                    let first_value = f64::from(first_pixel[channel]) / 255.0;
                    let second_value = f64::from(second_pixel[channel]) / 255.0;
                    let value = k1 * first_value * second_value
                        + k2 * first_value
                        + k3 * second_value
                        + k4;
                    destination[channel] =
                        (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
            _ => {
                let first_alpha = f64::from(first_pixel[3]) / 255.0;
                let second_alpha = f64::from(second_pixel[3]) / 255.0;
                let (first_factor, second_factor) = match operator {
                    CompositeOperator::Over => (1.0, 1.0 - first_alpha),
                    CompositeOperator::In => (second_alpha, 0.0),
                    CompositeOperator::Out => (1.0 - second_alpha, 0.0),
                    CompositeOperator::Atop => (second_alpha, 1.0 - first_alpha),
                    CompositeOperator::Xor => (1.0 - second_alpha, 1.0 - first_alpha),
                    CompositeOperator::Arithmetic { .. } => unreachable!(),
                };
                let output_alpha = (first_alpha * first_factor
                    + second_alpha * second_factor)
                    .clamp(0.0, 1.0);

                for channel in 0..3 {
                    let first_value = f64::from(first_pixel[channel]) / 255.0
                        * first_alpha
                        * first_factor;
                    let second_value = f64::from(second_pixel[channel]) / 255.0
                        * second_alpha
                        * second_factor;
                    destination[channel] = if output_alpha > 0.0 {
                        ((first_value + second_value) / output_alpha * 255.0)
                            .round()
                            .clamp(0.0, 255.0) as u8
                    } else {
                        0
                    };
                }
                destination[3] = (output_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    output.recompute_may_have_transparency();
    output
}

fn merge(
    inputs: &[FilterInput],
    source: &Pixmap,
    results: &HashMap<String, Pixmap>,
    width: u16,
    height: u16,
) -> Result<Pixmap, String> {
    let mut output = Pixmap::new(width, height);
    for input in inputs {
        let layer = resolve(input, source, results, width, height)?;
        output = composite(&layer, &output, CompositeOperator::Over, width, height);
    }
    Ok(output)
}

fn color_matrix(pixmap: &mut Pixmap, matrix: &[f64; 20]) {
    for pixel in pixmap.data_as_u8_slice_mut().chunks_exact_mut(4) {
        let input = [
            f64::from(pixel[0]) / 255.0,
            f64::from(pixel[1]) / 255.0,
            f64::from(pixel[2]) / 255.0,
            f64::from(pixel[3]) / 255.0,
        ];
        let mut output = [0.0; 4];
        for row in 0..4 {
            output[row] = (matrix[row * 5] * input[0]
                + matrix[row * 5 + 1] * input[1]
                + matrix[row * 5 + 2] * input[2]
                + matrix[row * 5 + 3] * input[3]
                + matrix[row * 5 + 4])
                .clamp(0.0, 1.0);
        }
        for channel in 0..4 {
            pixel[channel] = (output[channel] * 255.0).round() as u8;
        }
    }
    pixmap.recompute_may_have_transparency();
}

fn component_transfer(
    pixmap: &mut Pixmap,
    red: &TransferFunction,
    green: &TransferFunction,
    blue: &TransferFunction,
    alpha: &TransferFunction,
) {
    for pixel in pixmap.data_as_u8_slice_mut().chunks_exact_mut(4) {
        pixel[0] = (transfer(red, f64::from(pixel[0]) / 255.0) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        pixel[1] = (transfer(green, f64::from(pixel[1]) / 255.0) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        pixel[2] = (transfer(blue, f64::from(pixel[2]) / 255.0) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        pixel[3] = (transfer(alpha, f64::from(pixel[3]) / 255.0) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    pixmap.recompute_may_have_transparency();
}

fn transfer(function: &TransferFunction, input: f64) -> f64 {
    match function {
        TransferFunction::Identity => input,
        TransferFunction::Table(values) => sample_table(values, input, true),
        TransferFunction::Discrete(values) => sample_table(values, input, false),
        TransferFunction::Linear { slope, intercept } =>
            (*slope * input + *intercept).clamp(0.0, 1.0),
        TransferFunction::Gamma {
            amplitude,
            exponent,
            offset,
        } => (*amplitude * input.powf(*exponent) + *offset).clamp(0.0, 1.0),
    }
}

fn sample_table(values: &[f64], input: f64, linear: bool) -> f64 {
    if values.is_empty() {
        return input;
    }
    if values.len() == 1 {
        return values[0].clamp(0.0, 1.0);
    }

    if !linear {
        let index = (input.clamp(0.0, 0.999_999) * values.len() as f64) as usize;
        return values[index].clamp(0.0, 1.0);
    }

    let position = input.clamp(0.0, 1.0) * (values.len() - 1) as f64;
    let first = position.floor() as usize;
    let second = (first + 1).min(values.len() - 1);
    let fraction = position - first as f64;
    (values[first] * (1.0 - fraction) + values[second] * fraction).clamp(0.0, 1.0)
}

fn morphology(
    source: &Pixmap,
    operator: MorphologyOperator,
    radius_x: i32,
    radius_y: i32,
    width: u16,
    height: u16,
) -> Pixmap {
    let width_usize = usize::from(width);
    let height_usize = usize::from(height);
    let mut output = Pixmap::new(width, height);
    let source_data = source.data_as_u8_slice();
    let output_data = output.data_as_u8_slice_mut();

    for y in 0..height_usize {
        for x in 0..width_usize {
            for channel in 0..4 {
                let mut value = match operator {
                    MorphologyOperator::Erode => 255u8,
                    MorphologyOperator::Dilate => 0u8,
                };
                for offset_y in -radius_y..=radius_y {
                    for offset_x in -radius_x..=radius_x {
                        let sample_x = (x as i32 + offset_x)
                            .clamp(0, width_usize as i32 - 1)
                            as usize;
                        let sample_y = (y as i32 + offset_y)
                            .clamp(0, height_usize as i32 - 1)
                            as usize;
                        let sample =
                            source_data[(sample_y * width_usize + sample_x) * 4 + channel];
                        value = match operator {
                            MorphologyOperator::Erode => value.min(sample),
                            MorphologyOperator::Dilate => value.max(sample),
                        };
                    }
                }
                output_data[(y * width_usize + x) * 4 + channel] = value;
            }
        }
    }

    output.recompute_may_have_transparency();
    output
}

fn gaussian_blur(
    pixmap: &mut Pixmap,
    sigma_x: f64,
    sigma_y: f64,
    width: u16,
    height: u16,
) {
    let radius_x = (sigma_x * 3.0).ceil().clamp(0.0, 64.0) as i32;
    let radius_y = (sigma_y * 3.0).ceil().clamp(0.0, 64.0) as i32;
    if radius_x == 0 && radius_y == 0 {
        return;
    }

    let mut data = pixmap.data_as_u8_slice().to_vec();
    if radius_x > 0 {
        data = blur_axis(
            &data,
            usize::from(width),
            usize::from(height),
            radius_x,
            true,
        );
    }
    if radius_y > 0 {
        data = blur_axis(
            &data,
            usize::from(width),
            usize::from(height),
            radius_y,
            false,
        );
    }
    pixmap.data_as_u8_slice_mut().copy_from_slice(&data);
    pixmap.recompute_may_have_transparency();
}

fn blur_axis(
    source: &[u8],
    width: usize,
    height: usize,
    radius: i32,
    horizontal: bool,
) -> Vec<u8> {
    let sigma = (radius as f64 / 3.0).max(0.333);
    let mut weights = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut total = 0.0;
    for offset in -radius..=radius {
        let weight = (-((offset * offset) as f64) / (2.0 * sigma * sigma)).exp();
        weights.push(weight);
        total += weight;
    }
    for weight in &mut weights {
        *weight /= total;
    }

    let mut output = vec![0; source.len()];
    for y in 0..height {
        for x in 0..width {
            for channel in 0..4 {
                let mut accumulated = 0.0;
                for (offset, weight) in (-radius..=radius).zip(weights.iter().copied()) {
                    let (sample_x, sample_y) = if horizontal {
                        (
                            (x as i32 + offset).clamp(0, width as i32 - 1) as usize,
                            y,
                        )
                    } else {
                        (
                            x,
                            (y as i32 + offset).clamp(0, height as i32 - 1) as usize,
                        )
                    };
                    accumulated +=
                        f64::from(source[(sample_y * width + sample_x) * 4 + channel]) * weight;
                }
                output[(y * width + x) * 4 + channel] =
                    accumulated.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    output
}

fn offset_pixmap(pixmap: &mut Pixmap, dx: i32, dy: i32, width: u16, height: u16) {
    let width_usize = usize::from(width);
    let height_usize = usize::from(height);
    let source = pixmap.data_as_u8_slice().to_vec();
    let destination = pixmap.data_as_u8_slice_mut();
    destination.fill(0);

    for y in 0..height_usize {
        for x in 0..width_usize {
            let destination_x = x as i32 + dx;
            let destination_y = y as i32 + dy;
            if destination_x >= 0
                && destination_y >= 0
                && destination_x < width_usize as i32
                && destination_y < height_usize as i32
            {
                let source_index = (y * width_usize + x) * 4;
                let destination_index =
                    (destination_y as usize * width_usize + destination_x as usize) * 4;
                destination[destination_index..destination_index + 4]
                    .copy_from_slice(&source[source_index..source_index + 4]);
            }
        }
    }
    pixmap.recompute_may_have_transparency();
}

fn clip_region(pixmap: &mut Pixmap, region: FilterRegion, width: u16, height: u16) {
    let left = region.x.floor() as i32;
    let top = region.y.floor() as i32;
    let right = (region.x + region.width).ceil() as i32;
    let bottom = (region.y + region.height).ceil() as i32;
    let width_usize = usize::from(width);
    let height_usize = usize::from(height);
    let data = pixmap.data_as_u8_slice_mut();

    for y in 0..height_usize {
        for x in 0..width_usize {
            if x as i32 < left
                || x as i32 >= right
                || y as i32 < top
                || y as i32 >= bottom
            {
                let index = (y * width_usize + x) * 4;
                data[index..index + 4].fill(0);
            }
        }
    }
    pixmap.recompute_may_have_transparency();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_alpha_zeroes_rgb() {
        let mut pixmap = Pixmap::new(1, 1);
        pixmap
            .data_as_u8_slice_mut()
            .copy_from_slice(&[10, 20, 30, 40]);
        let alpha = source_alpha(&pixmap, 1, 1);
        assert_eq!(alpha.data_as_u8_slice(), &[0, 0, 0, 40]);
    }

    #[test]
    fn flood_fills() {
        let pixmap = flood(
            Rgba {
                r: 1,
                g: 2,
                b: 3,
                a: 4,
            },
            2,
            1,
        );
        assert_eq!(pixmap.data_as_u8_slice(), &[1, 2, 3, 4, 1, 2, 3, 4]);
    }

    #[test]
    fn unsupported_bypass_keeps_previous_result() {
        let mut source = Pixmap::new(1, 1);
        source
            .data_as_u8_slice_mut()
            .copy_from_slice(&[10, 20, 30, 255]);
        let graph = FilterGraph {
            region: FilterRegion::full(1, 1),
            output: FilterInput::Named("u".into()),
            nodes: vec![
                FilterNode {
                    result: "a".into(),
                    op: FilterPrimitive::Offset {
                        input: FilterInput::SourceGraphic,
                        dx: 0.0,
                        dy: 0.0,
                    },
                },
                FilterNode {
                    result: "u".into(),
                    op: FilterPrimitive::Unsupported {
                        name: "feNoise".into(),
                        input: FilterInput::Named("a".into()),
                    },
                },
            ],
        };
        let output = execute_filter_graph(&source, &graph, 1, 1).unwrap();
        assert_eq!(output.data_as_u8_slice(), source.data_as_u8_slice());
    }
}
