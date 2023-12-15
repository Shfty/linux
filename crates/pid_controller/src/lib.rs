use std::ops::{Add, Div, Mul, Sub};

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Hash)]
pub struct PidParameters<T> {
    pub proportional_factor: T,
    pub integral_factor: T,
    pub derivative_factor: T,
}

impl Default for PidParameters<f32> {
    fn default() -> Self {
        PidParameters {
            proportional_factor: 1.0,
            integral_factor: 1.0,
            derivative_factor: 1.0,
        }
    }
}

impl Default for PidParameters<f64> {
    fn default() -> Self {
        PidParameters {
            proportional_factor: 1.0,
            integral_factor: 1.0,
            derivative_factor: 1.0,
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, PartialOrd, Hash)]
pub struct PidInputs<T> {
    pub setpoint: T,
    pub measured_value: T,
}

impl<T> PidInputs<T>
where
    T: Copy + Sub<T, Output = T>,
{
    pub fn error(&self) -> T {
        self.setpoint - self.measured_value
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, PartialOrd)]
pub struct PidOutputs<T> {
    pub proportional: T,
    pub integral: T,
    pub derivative: T,
}

impl<T> PidOutputs<T>
where
    T: Copy + Add<T, Output = T>,
{
    pub fn total(&self) -> T {
        self.proportional + self.integral + self.derivative
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, PartialOrd)]
pub struct PidState<T> {
    pub error: T,
    pub integral: T,
}

#[derive(Debug, Copy, Clone)]
pub struct PidController<T = f64> {
    pub params: PidParameters<T>,
    pub inputs: PidInputs<T>,
    pub outputs: PidOutputs<T>,

    pub state: PidState<T>,
}

impl Default for PidController<f32> {
    fn default() -> Self {
        PidController {
            params: Default::default(),
            inputs: Default::default(),
            outputs: Default::default(),
            state: Default::default(),
        }
    }
}

impl Default for PidController<f64> {
    fn default() -> Self {
        PidController {
            params: Default::default(),
            inputs: Default::default(),
            outputs: Default::default(),
            state: Default::default(),
        }
    }
}

impl<T> PidController<T>
where
    Self: Default,
{
    pub fn new(params: PidParameters<T>) -> Self {
        PidController {
            params,
            ..Default::default()
        }
    }
}

impl<T> PidController<T>
where
    T: Copy + Add<T, Output = T> + Sub<T, Output = T> + Mul<T, Output = T> + Div<T, Output = T>,
{
    pub fn tick(&mut self, delta: T) {
        let error = self.inputs.error();

        self.state.integral = self.state.integral + error * delta;
        let derivative = (error - self.state.error) / delta;

        self.outputs.proportional = error * self.params.proportional_factor;
        self.outputs.integral = self.state.integral * self.params.integral_factor;
        self.outputs.derivative = derivative * self.params.derivative_factor;

        self.state.error = error;
    }
}
