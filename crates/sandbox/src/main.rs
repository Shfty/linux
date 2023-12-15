use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    collections::VecDeque,
    io, thread,
    time::{Duration, Instant},
};
use tui::{
    backend::CrosstermBackend,
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Chart, Dataset, GraphType},
    Terminal,
};

use pid_controller::*;

const BUFFER_LEN: usize = 400;
type Datapoint = (f64, f64);

fn dataset<'a>(data: &'a [(f64, f64)], name: &'a str, color: Color) -> Dataset<'a> {
    Dataset::default()
        .name(name)
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Scatter)
        .style(Style::default().fg(color))
        .data(data)
}

fn chart<'a>(
    datasets: Vec<Dataset<'a>>,
    half_len_string: &'a str,
    full_len_string: &'a str,
) -> Chart<'a> {
    Chart::new(datasets)
        .block(Block::default().title("Chart"))
        .x_axis(
            Axis::default()
                .title(Span::styled("Time", Style::default().fg(Color::Red)))
                .style(Style::default().fg(Color::White))
                .bounds([0.0, BUFFER_LEN as f64])
                .labels(
                    ["0.0", half_len_string, full_len_string]
                        .iter()
                        .cloned()
                        .map(Span::from)
                        .collect(),
                ),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled("Value", Style::default().fg(Color::Red)))
                .style(Style::default().fg(Color::White))
                .bounds([-1.1, 1.1])
                .labels(
                    ["-1.5", "0.0", "1.5"]
                        .iter()
                        .cloned()
                        .map(Span::from)
                        .collect(),
                ),
        )
}

fn data_buffer() -> VecDeque<(f64, f64)> {
    let mut buf: VecDeque<Datapoint> = Default::default();
    buf.resize(BUFFER_LEN, (0.0, 0.0));
    buf
}

fn push_data(data: &mut VecDeque<(f64, f64)>, value: f64) {
    // Add new acceleration data
    for acceleration in data.iter_mut() {
        acceleration.0 -= 1.0;
    }
    data.push_back((data.len() as f64, value));
    while data.len() > BUFFER_LEN {
        data.pop_front();
    }

    data.make_contiguous();
}

fn main() -> Result<(), io::Error> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let half_len_string = (BUFFER_LEN as f64 / 2.0).to_string();
    let full_len_string = (BUFFER_LEN as f64).to_string();

    let mut acceleration_data = data_buffer();
    let mut input_data = data_buffer();
    let mut output_data = data_buffer();
    let mut p_data = data_buffer();
    let mut i_data = data_buffer();
    let mut d_data = data_buffer();

    let mut pid = PidController::<f64>::new(PidParameters {
        proportional_factor: 12.0,
        integral_factor: 8.0,
        derivative_factor: 12.0,
    });

    let mut position = 0.0;
    let mut velocity = 0.0;

    let mut acceleration;

    let ts_start = Instant::now();
    let mut ts_prev = Instant::now();
    loop {
        // Update time
        let now = Instant::now();
        let time_total = now.duration_since(ts_start).as_secs_f64();
        let time_delta = now.duration_since(ts_prev).as_secs_f64();
        ts_prev = now;

        // Integrate position
        let accel = ((time_total * 3.0).sin() * 0.5) - ((time_total * 2.0).sin() * 0.25);
        acceleration = accel + pid.outputs.total();

        velocity += acceleration * time_delta;

        position += velocity * time_delta;

        // Process events
        if event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(event) => match event.code {
                    event::KeyCode::Char('c') => {
                        if event.modifiers.contains(KeyModifiers::CONTROL) {
                            break;
                        }
                    }
                    _ => (),
                },
                _ => (),
            }
        }

        // Add new acceleration data
        push_data(&mut acceleration_data, acceleration);

        // Add new input data
        push_data(&mut input_data, position);

        // Update PID controller
        pid.inputs.measured_value = input_data.back().unwrap().1;
        pid.tick(time_delta);

        // Add new output data
        push_data(&mut output_data, pid.outputs.total());
        push_data(&mut p_data, pid.outputs.proportional);
        push_data(&mut i_data, pid.outputs.integral);
        push_data(&mut d_data, pid.outputs.derivative);

        // Render
        let (title_acceleration, title_position, title_p, title_i, title_d, title_output) = (
            format!("Acceleration: {:+.5}", acceleration),
            format!("    Position: {:+.5}", position),
            format!("           P: {:+.5}", pid.outputs.proportional),
            format!("           I: {:+.5}", pid.outputs.integral),
            format!("           D: {:+.5}", pid.outputs.derivative),
            format!("       Total: {:+.5}", pid.outputs.total()),
        );

        let datasets = vec![
            dataset(
                &acceleration_data.as_slices().0,
                &title_acceleration,
                Color::Yellow,
            ),
            dataset(&input_data.as_slices().0, &title_position, Color::White),
            dataset(&p_data.as_slices().0, &title_p, Color::Red),
            dataset(&i_data.as_slices().0, &title_i, Color::Magenta),
            dataset(&d_data.as_slices().0, &title_d, Color::Blue),
            dataset(&output_data.as_slices().0, &title_output, Color::Cyan),
        ];

        terminal.draw(|f| {
            let size = f.size();
            f.render_widget(chart(datasets, &half_len_string, &full_len_string), size);
        })?;

        thread::sleep(Duration::from_millis(8));
    }

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
