use crate::internal::*;
use ndarray::prelude::*;

macro_rules! r {
    ($($path:ident)::* ($dt:expr) ($($args:expr),*)) => {
        match $dt {
            DatumType::U8   => $($path)::*::<u8,_>($($args),*),
            DatumType::U16  => $($path)::*::<u16,_>($($args),*),
            DatumType::I8   => $($path)::*::<i8,_>($($args),*),
            DatumType::I16  => $($path)::*::<i16,_>($($args),*),
            DatumType::I32  => $($path)::*::<i32,_>($($args),*),
            DatumType::I64  => $($path)::*::<i64,_>($($args),*),
            DatumType::F32  => $($path)::*::<f32,_>($($args),*),
            DatumType::F64  => $($path)::*::<f64,_>($($args),*),
            _ => bail!("{:?} is not a number", $dt)
        }
    }
}

#[derive(Clone, Copy, Debug, Hash)]
pub enum Reducer {
    Max,
    Min,
    Prod,
    Sum,
}

impl Reducer {
    pub fn reduce(&self, axes: &[usize], input: &Tensor) -> TractResult<Tensor> {
        use Reducer::*;
        let dt = input.datum_type();
        let output_shape: Vec<usize> = input
            .shape()
            .iter()
            .enumerate()
            .map(|(ax, &d)| if axes.contains(&ax) { 1 } else { d })
            .collect();
        Ok(unsafe {
            match self {
                Min => r!(Self::reduce_t(dt)(self, axes, &output_shape, input, min_t)),
                Max => r!(Self::reduce_t(dt)(self, axes, &output_shape, input, max_t)),
                Prod => r!(Self::reduce_t(dt)(self, axes, &output_shape, input, prod_t)),
                Sum => r!(Self::reduce_t(dt)(self, axes, &output_shape, input, sum_t)),
            }
        })
    }

    unsafe fn reduce_t<T, F>(
        &self,
        axes: &[usize],
        output_shape: &[usize],
        input: &Tensor,
        f: F,
    ) -> Tensor
    where
        F: for<'a> Fn(ArrayViewD<'a, T>) -> T,
        T: Copy + Datum,
    {
        use ndarray::*;
        let input = input.to_array_view_unchecked::<T>();
        let result = Array::from_shape_fn(output_shape, |coords| {
            let slice_spec: Vec<SliceOrIndex> = coords
                .slice()
                .iter()
                .enumerate()
                .map(|(ax, &d)| if axes.contains(&ax) { (..).into() } else { d.into() })
                .collect();
            let slice_info = SliceInfo::new(&slice_spec).unwrap();
            let slice = input.slice(slice_info.as_ref());
            f(slice)
        });
        result.into_tensor()
    }
}

fn max_t<'a, T>(v: ArrayViewD<'a, T>) -> T
where
    T: Copy + Datum + num_traits::Bounded + ::std::cmp::PartialOrd,
{
    v.fold(T::min_value(), |acc, &v| if acc > v { acc } else { v })
}

fn min_t<'a, T>(v: ArrayViewD<'a, T>) -> T
where
    T: Copy + Datum + num_traits::Bounded + ::std::cmp::PartialOrd,
{
    v.fold(T::max_value(), |acc, &v| if acc < v { acc } else { v })
}

fn prod_t<'a, T>(v: ArrayViewD<'a, T>) -> T
where
    T: Copy + Datum + num_traits::One,
{
    v.fold(T::one(), |acc, &v| acc * v)
}

fn sum_t<'a, T>(v: ArrayViewD<'a, T>) -> T
where
    T: Copy + Datum + num_traits::Zero,
{
    v.scalar_sum()
}

#[derive(Clone, Debug, new, Hash)]
pub struct Reduce {
    axes: TVec<usize>,
    reducer: Reducer,
}

tract_linalg::impl_dyn_hash!(Reduce);

impl Op for Reduce {
    fn name(&self) -> Cow<str> {
        format!("Reduce<{:?}>", self.reducer).into()
    }
    fn info(&self) -> TractResult<Vec<String>> {
        Ok(vec![format!("axes: {:?}", self.axes)])
    }
    op_core_mir!();
    canonic!();
    op_as_typed_op!();
    op_as_pulsed_op!();
}

impl StatelessOp for Reduce {
    fn eval(&self, inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        Ok(tvec!(self.reducer.reduce(&*self.axes, inputs[0].as_ref())?.into_arc_tensor()))
    }
}

impl TypedOp for Reduce {
    as_op!();
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        let mut shape: TVec<_> = inputs[0].shape.to_tvec();
        for &ax in &self.axes {
            shape[ax] = 1.to_dim();
        }
        Ok(tvec!(TypedFact::dt_shape(inputs[0].datum_type, &*shape)?))
    }

    #[allow(unused_variables)]
    fn invariants(&self, model: &TypedModel, node: &TypedNode) -> TractResult<Invariants> {
        let input = model.outlet_fact(node.inputs[0])?;
        let axes = (0..input.rank())
            .filter(|axis| !self.axes.contains(axis))
            .map(|axis| AxisInfo::simple(axis))
            .collect::<TVec<_>>();
        Ok(axes.into())
    }

    fn change_axes(
        &self,
        model: &TypedModel,
        node: &TypedNode,
        _io: InOut,
        change: &AxisOp,
    ) -> TractResult<Option<AxisChangeConsequence>> {
        let mut axes = tvec!();
        for reduced in &self.axes {
            if let Some(axis) = change.transform_axis(*reduced) {
                axes.push(axis);
            } else {
                return Ok(None);
            }
        }
        let op = Some(Box::new(Self { axes, ..self.clone() }) as _);
        Ok(Some(AxisChangeConsequence::new(model, node, op, change)))
    }

    fn pulsify(
        &self,
        _source: &NormalizedModel,
        node: &NormalizedNode,
        target: &mut PulsedModel,
        mapping: &HashMap<OutletId, OutletId>,
        _pulse: usize,
    ) -> TractResult<TVec<OutletId>> {
        let input = mapping[&node.inputs[0]];
        let axis = target.outlet_fact(input)?.axis;
        if self.axes.contains(&axis) {
            bail!("Can not reduce over streaming axis");
        }
        target.wire_node(&*node.name, self.clone(), &[input])
    }
}

impl PulsedOp for Reduce {
    fn pulsed_output_facts(&self, inputs: &[&PulsedFact]) -> TractResult<TVec<PulsedFact>> {
        let mut fact = inputs[0].clone();
        for &ax in &self.axes {
            fact.shape[ax] = 1;
        }
        Ok(tvec!(fact))
    }

    as_op!();
    pulsed_op_to_typed_op!();
}
