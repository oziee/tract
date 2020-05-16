use crate::internal::*;
use crate::pulse::PulsedFact;
use ndarray::*;

#[derive(Debug, new, Clone)]
struct DelayState {
    buffer: Tensor,
}

impl DelayState {
    pub fn eval_t<T: Datum>(&mut self, op: &Delay, input: Arc<Tensor>) -> TractResult<Arc<Tensor>> {
        let axis = Axis(op.axis);
        let input = input.to_array_view::<T>()?;
        let mut buffer = self.buffer.to_array_view_mut::<T>()?;

        let buffered = op.delay + op.overlap;
        let input_pulse = input.shape()[op.axis];
        let output_pulse = input_pulse + op.overlap;
        let mut output_shape: TVec<usize> = input.shape().into();
        output_shape[op.axis] = output_pulse;
        // build output
        let output = if op.delay < input_pulse {
            let mut output = unsafe { Tensor::uninitialized::<T>(&*output_shape)? };
            let from_input = input_pulse - op.delay;
            let from_buffer = output_pulse - from_input;
            output
                .to_array_view_mut::<T>()?
                .slice_axis_mut(axis, Slice::from(..from_buffer))
                .assign(&buffer.slice_axis(axis, Slice::from(..from_buffer)));
            output
                .to_array_view_mut::<T>()?
                .slice_axis_mut(axis, Slice::from(from_buffer..))
                .assign(&input.slice_axis(axis, Slice::from(..from_input)));
            output
        } else {
            buffer.slice_axis(axis, Slice::from(..output_pulse)).to_owned().into_tensor()
        };
        // maintain buffer
        if buffered < input_pulse {
            buffer.assign(&input.slice_axis(axis, Slice::from((input_pulse - buffered)..)));
        } else {
            let stride = buffer.strides()[op.axis] as usize * input_pulse;
            buffer.as_slice_mut().unwrap().rotate_left(stride);
            buffer.slice_axis_mut(axis, Slice::from((buffered - input_pulse)..)).assign(&input);
        }
        let output = output.into_arc_tensor();
        Ok(output)
    }
}

impl OpState for DelayState {
    fn eval(
        &mut self,
        _state: &mut SessionState,
        op: &dyn Op,
        mut inputs: TVec<Arc<Tensor>>,
    ) -> TractResult<TVec<Arc<Tensor>>> {
        let input = args_1!(inputs);
        let op = op.downcast_ref::<Delay>().ok_or("Wrong Op type")?;
        Ok(tvec!(dispatch_datum!(Self::eval_t(input.datum_type())(self, op, input))?))
    }
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Delay {
    datum_type: DatumType,
    buffer_shape: TVec<usize>,
    axis: usize,
    delay: usize,
    overlap: usize,
}

tract_linalg::impl_dyn_hash!(Delay);

impl Delay {
    pub fn new(input_fact: &PulsedFact, delay: usize, overlap: usize) -> Delay {
        let axis = input_fact.axis;
        let mut buffer_shape = input_fact.shape.clone();
        buffer_shape[axis] = delay + overlap;
        Delay { datum_type: input_fact.datum_type, buffer_shape, axis, delay, overlap }
    }
}

impl Op for Delay {
    fn name(&self) -> Cow<str> {
        "Delay".into()
    }

    fn info(&self) -> TractResult<Vec<String>> {
        Ok(vec![
            format!("axis: {} delay: {} overlap: {}", self.axis, self.delay, self.overlap),
            format!("buffer: {:?} {:?}", self.buffer_shape, self.datum_type),
        ])
    }

    canonic!();
    impl_op_same_as!();
    op_as_typed_op!();
    op_as_pulsed_op!();
}

impl StatefullOp for Delay {
    fn state(
        &self,
        _session: &mut SessionState,
        _node_id: usize,
    ) -> TractResult<Option<Box<dyn OpState>>> {
        let buffer = unsafe { Tensor::uninitialized_dt(self.datum_type, &*self.buffer_shape)? };
        Ok(Some(Box::new(DelayState { buffer })))
    }
}

impl TypedOp for Delay {
    as_op!();

    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        let mut fact = inputs[0].clone();
        fact.shape.set_dim(self.axis, fact.shape.dim(self.axis) + self.overlap)?;
        Ok(tvec!(fact))
    }

    fn cost(&self, _inputs: &[&TypedFact]) -> TractResult<TVec<(Cost, TDim)>> {
        Ok(tvec!((
            Cost::Buffer(self.datum_type),
            self.buffer_shape.iter().product::<usize>().to_dim()
        )))
    }
}

impl PulsedOp for Delay {
    fn pulsed_output_facts(&self, inputs: &[&PulsedFact]) -> TractResult<TVec<PulsedFact>> {
        let mut fact = inputs[0].clone();
        fact.shape[self.axis] += self.overlap;
        fact.delay += self.delay + self.overlap;
        Ok(tvec!(fact))
    }

    as_op!();
    pulsed_op_to_typed_op!();
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::*;

    fn test_pulse_delay_over(pulse: usize, delay: usize, overlap: usize) {
        let mut model = PulsedModel::default();
        let fact1 = PulsedFact {
            datum_type: u8::datum_type(),
            shape: tvec![pulse],
            axis: 0,
            dim: TDim::s(),
            delay: 0,
        };
        let source = model.add_source("source", fact1.clone()).unwrap();
        model.wire_node("delay", Delay::new(&fact1, delay, overlap), &[source]).unwrap();
        model.auto_outputs().unwrap();

        let plan = SimplePlan::new(model).unwrap();
        let mut state = crate::plan::SimpleState::new(plan).unwrap();

        for i in 0..5 {
            let input: Vec<u8> = (pulse * i..(pulse * (i + 1))).map(|a| a as u8).collect();
            let expect: Vec<u8> = (pulse * i..(pulse * (i + 1) + overlap))
                .map(|i| i.saturating_sub(delay + overlap) as u8)
                .collect();
            let output = state.run(tvec!(Tensor::from(arr1(&input)))).unwrap();
            let skip = (delay + overlap).saturating_sub(i * pulse).min(pulse + overlap);
            assert_eq!(&output[0].as_slice::<u8>().unwrap()[skip..], &expect[skip..]);
        }
    }

    #[test]
    fn sub_pulse() {
        test_pulse_delay_over(4, 1, 0);
    }

    #[test]
    fn supra_pulse() {
        test_pulse_delay_over(4, 5, 0);
    }

    #[test]
    fn sub_pulse_context() {
        test_pulse_delay_over(4, 0, 2);
    }

    #[test]
    fn supra_pulse_context() {
        test_pulse_delay_over(4, 0, 6);
    }

    #[test]
    fn test_two_delays() {
        let pulse = 4;
        let mut model = PulsedModel::default();
        let fact_0 = PulsedFact {
            datum_type: u8::datum_type(),
            shape: tvec![pulse],
            axis: 0,
            dim: TDim::s(),
            delay: 0,
        };
        let source = model.add_source("source", fact_0.clone()).unwrap();
        let delay_1 = model.wire_node("delay-1", Delay::new(&fact_0, 2, 0), &[source]).unwrap()[0];
        let fact_1 = model.outlet_fact(delay_1).unwrap().clone();
        let delay_2 = model.wire_node("delay-1", Delay::new(&fact_1, 2, 0), &[delay_1]).unwrap();
        model.set_output_outlets(&delay_2).unwrap();

        let plan = SimplePlan::new(model).unwrap();
        let mut state = crate::plan::SimpleState::new(plan).unwrap();

        for i in 0..5 {
            let input: Vec<u8> = (pulse * i..(pulse * (i + 1))).map(|a| a as u8).collect();
            let expect: Vec<u8> =
                (pulse * i..(pulse * (i + 1))).map(|i| i.saturating_sub(4) as u8).collect();
            let skip = 4usize.saturating_sub(i * pulse).min(pulse);
            let output = state.run(tvec!(Tensor::from(arr1(&input)))).unwrap();
            assert_eq!(&output[0].as_slice::<u8>().unwrap()[skip..], &expect[skip..]);
        }
    }
}
