use ndarray::*;

use super::*;

#[derive(Debug, Clone, new, Hash)]
pub struct LirScanOpParams {
    pub skip: usize,
    pub backward: bool,
    pub plan: Arc<TypedSimplePlan<TypedModel>>,
    pub input_mapping: Vec<InputMapping<usize>>,
    pub output_mapping: Vec<OutputMapping<usize, TDim>>,
}

#[derive(Debug, Clone, new, Hash)]
pub struct LirScan(Arc<LirScanOpParams>);

impl std::ops::Deref for LirScan {
    type Target = LirScanOpParams;
    fn deref(&self) -> &LirScanOpParams {
        &self.0
    }
}

tract_linalg::impl_dyn_hash!(LirScan);

impl Op for LirScan {
    fn name(&self) -> Cow<str> {
        "Scan".into()
    }

    fn nested_models(&self) -> Vec<(Cow<str>, &dyn Model, Vec<String>, Vec<String>)> {
        vec![(
            "loop".into(),
            self.plan.model(),
            self.input_mapping.iter().map(|m| format!("{:?}", m)).collect(),
            self.output_mapping.iter().map(|m| format!("{:?}", m)).collect(),
        )]
    }

    fn info(&self) -> TractResult<Vec<String>> {
        let mut lines = vec![];
        for (ix, im) in self.input_mapping.iter().enumerate() {
            lines.push(format!("Model input  #{}: {:?}", ix, im));
        }
        for (ix, om) in self.output_mapping.iter().enumerate() {
            lines.push(format!("Model output #{}: {:?}", ix, om));
        }
        Ok(lines)
    }

    op_core_lir!();
    op_as_typed_op!();
    not_a_pulsed_op!();
}

impl StatefullOp for LirScan {
    fn state(
        &self,
        _session: &mut SessionState,
        _node_id: usize,
    ) -> TractResult<Option<Box<dyn OpState>>> {
        Ok(Some(Box::new(State {
            mutable: MutableState {
                position: 0,
                hidden_state: tvec!(),
                model_state: TypedSimpleState::new(Arc::clone(&self.plan))?,
            },
            op: Arc::clone(&self.0),
        })))
    }
}

#[derive(Debug)]
struct State {
    op: Arc<LirScanOpParams>,
    mutable: MutableState,
}

#[derive(Debug)]
struct MutableState {
    position: usize,
    hidden_state: TVec<Tensor>,
    model_state: TypedSimpleState<TypedModel, Arc<TypedSimplePlan<TypedModel>>>,
}

impl MutableState {
    pub(super) fn slice_input_t<T: Datum>(
        &self,
        input: &Tensor,
        axis: usize,
        chunk_ix: usize,
        chunk_dim: usize,
        backward: bool,
    ) -> TractResult<Tensor> {
        let view = input.to_array_view::<T>()?;
        let full_len = view.shape()[axis];
        if backward {
            let mut shape: TVec<usize> = view.shape().into();
            shape[axis] = chunk_dim;
            let mut t = ArrayD::<T>::default(&*shape);
            for i in 0..chunk_dim {
                if chunk_dim * chunk_ix + i < full_len {
                    t.index_axis_mut(Axis(axis), chunk_dim - i - 1).assign(
                        &view.index_axis(Axis(axis), full_len - 1 - (chunk_ix * chunk_dim + i)),
                    );
                }
            }
            Ok(t.into_tensor())
        } else if (chunk_ix + 1) * chunk_dim > full_len {
            let remain = full_len - chunk_ix * chunk_dim;
            let mut shape: TVec<usize> = view.shape().into();
            shape[axis] = chunk_dim;
            let mut t = ArrayD::<T>::default(&*shape);
            t.slice_axis_mut(Axis(axis), (0..remain).into())
                .assign(&view.slice_axis(Axis(axis), (chunk_ix * chunk_dim..).into()));
            Ok(t.into_tensor())
        } else {
            Ok(view
                .slice_axis(Axis(axis), (chunk_ix * chunk_dim..(chunk_ix + 1) * chunk_dim).into())
                .to_owned()
                .into_tensor())
        }
    }

    pub(super) fn alloc_output_t<T: Datum + Default>(
        &self,
        shape: &[usize],
    ) -> TractResult<Tensor> {
        unsafe { Tensor::uninitialized::<T>(&shape) }
    }

    pub(super) fn assign_output_t<T: Datum + Default>(
        &self,
        output: &mut Tensor,
        axis: usize,
        element_value: &Tensor,
        i: usize,
        backward: bool,
    ) -> TractResult<()> {
        let mut view = output.to_array_view_mut::<T>()?;
        let full_len = view.shape()[axis];
        let element = element_value.to_array_view::<T>()?;
        let offset = if backward {
            full_len - 1 - i * element_value.shape()[axis]
        } else {
            i * element_value.shape()[axis]
        };
        let count = element_value.shape()[axis].min(view.shape()[axis] - offset);
        view.slice_axis_mut(Axis(axis), (offset..offset + count).into())
            .assign(&element.slice_axis(Axis(axis), (..count).into()));
        Ok(())
    }
}

impl OpState for State {
    fn eval(
        &mut self,
        _session: &mut SessionState,
        _op: &dyn Op,
        inputs: TVec<Arc<Tensor>>,
    ) -> TractResult<TVec<Arc<Tensor>>> {
        let State { op, ref mut mutable } = self;
        // initialize state at first pass
        if mutable.hidden_state.len() == 0 {
            for input in &op.input_mapping {
                if let InputMapping::State { initializer } = input {
                    mutable.hidden_state.push(match initializer {
                        StateInitializer::FromInput(slot) => (*inputs[*slot]).to_owned(),
                        StateInitializer::Value(v) => (**v).to_owned(),
                    });
                }
            }
        }

        let iters = {
            let (outside_slot, axis, chunk) = op
                .input_mapping
                .iter()
                .filter_map(|it| match it {
                    InputMapping::Scan { axis, slot, chunk } => Some((*slot, *axis, *chunk)),
                    _ => None,
                })
                .next()
                .unwrap();
            inputs[outside_slot].shape()[axis].div_ceil(chunk)
        };

        let mut outputs = tvec!();
        for (ix, output) in op.output_mapping.iter().enumerate() {
            if let Some(slot) = output.full_slot {
                let fact = op.plan.model().output_fact(ix)?;
                let mut shape: TVec<usize> = fact.shape.as_finite().unwrap().into();
                let scanning_dim = output
                    .full_dim_hint
                    .as_ref()
                    .and_then(|d| d.to_integer().ok().map(|i| i as usize))
                    .unwrap_or(shape[output.axis] * iters);
                shape[output.axis] = scanning_dim;
                let t = dispatch_datum!(MutableState::alloc_output_t(fact.datum_type)(
                    mutable, &*shape
                ))?;
                outputs.push((slot, t));
            }
            if let Some(slot) = output.last_value_slot {
                outputs.push((slot, Tensor::default()));
            }
        }
        outputs.sort_by_key(|a| a.0);
        let mut outputs: TVec<Tensor> = outputs.into_iter().map(|(_slot, v)| v).collect();

        for i in 0..iters {
            mutable.position += 1;
            if mutable.position <= op.skip {
                continue;
            }
            mutable.hidden_state.reverse();

            let iter_inputs: TVec<Tensor> = op
                .input_mapping
                .iter()
                .map(|m| {
                    Ok(match m {
                        InputMapping::State { .. } => Some(mutable.hidden_state.pop().unwrap()),
                        InputMapping::Scan { slot, axis, chunk } => Some(dispatch_datum!(
                            MutableState::slice_input_t(inputs[*slot].datum_type())(
                                mutable,
                                inputs[*slot].as_ref(),
                                *axis,
                                i,
                                *chunk,
                                op.backward
                            )
                        )?),
                        InputMapping::Full { slot } => Some(inputs[*slot].clone().into_tensor()),
                    })
                })
                .collect::<TractResult<Vec<_>>>()?
                .into_iter()
                .filter_map(|x| x)
                .collect();

            trace!("iter_inputs: {:?}", iter_inputs);
            let iter_outputs =
                mutable.model_state.run(iter_inputs).chain_err(|| "Evaluating inner body")?;
            trace!("iter_outputs: {:?}", iter_outputs);

            for (v, mapping) in iter_outputs.into_iter().zip(&op.output_mapping) {
                if let Some(slot) = mapping.full_slot {
                    dispatch_datum!(MutableState::assign_output_t(v.datum_type())(
                        mutable,
                        &mut outputs[slot],
                        mapping.axis,
                        v.as_ref(),
                        i,
                        op.backward
                    ))?;
                }
                if i == iters - 1 {
                    if let Some(slot) = mapping.last_value_slot {
                        outputs[slot] = v.clone().into_tensor();
                    }
                }
                if mapping.state {
                    mutable.hidden_state.push(v.into_tensor());
                }
            }
        }

        Ok(outputs.into_iter().map(Arc::new).collect())
    }
}

impl TypedOp for LirScan {
    as_op!();

    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        let mut outputs = tvec!();
        let iters = {
            let (outside_slot, axis, chunk) = self
                .input_mapping
                .iter()
                .filter_map(|it| match it {
                    InputMapping::Scan { axis, slot, chunk } => Some((*slot, *axis, *chunk)),
                    _ => None,
                })
                .next()
                .unwrap();
            inputs[outside_slot].shape.dim(axis).div_ceil(chunk as u32)
        };
        for (ix, output) in self.output_mapping.iter().enumerate() {
            let fact = self.plan.model().output_fact(ix)?;
            if let Some(slot) = output.last_value_slot {
                outputs.push((slot, TypedFact::dt_shape(fact.datum_type, fact.shape.clone())?));
            }
            if let Some(slot) = output.full_slot {
                let mut shape = fact.shape.clone();
                let scanning_dim = output
                    .full_dim_hint
                    .clone()
                    .unwrap_or(shape.dim(output.axis).maybe_mul(&iters)?);
                shape.set_dim(output.axis, scanning_dim)?;
                outputs.push((slot, TypedFact::dt_shape(fact.datum_type, shape)?));
            }
        }
        outputs.sort_by_key(|a| a.0);
        let outputs: TVec<_> = outputs.into_iter().map(|(_slot, v)| v).collect();
        Ok(outputs)
    }

    fn nested_model_multipliers(&self, inputs: &[&TypedFact]) -> Vec<(Cow<str>, f64)> {
        let iters = {
            let (outside_slot, axis, chunk) = self
                .input_mapping
                .iter()
                .filter_map(|it| match it {
                    InputMapping::Scan { axis, slot, chunk } => Some((*slot, *axis, *chunk)),
                    _ => None,
                })
                .next()
                .unwrap();
            let outside_dim = inputs[outside_slot].shape.dim(axis);
            if let Ok(i) = outside_dim.to_integer() {
                i as f64 / chunk as f64
            } else {
                let big = 1_000_000;
                let dominator =
                    inputs[outside_slot].shape.dim(axis).eval(big).unwrap() as f64 / big as f64;
                dominator / chunk as f64
            }
        };
        vec![("loop".into(), iters as f64)]
    }
}
