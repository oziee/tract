use crate::internal::*;
use num_traits::Zero;
use crate::ops::element_wise::ElementWiseOp;
use num_traits::AsPrimitive;
use tract_linalg::lut::Lut;

#[derive(Clone, Debug)]
pub struct QParams {
    pub c_datum_type: DatumType,
    pub zero_point_a: Option<Arc<Tensor>>,
    pub zero_point_b: Option<Arc<Tensor>>,
    pub zero_point_c: Option<Arc<Tensor>>,
    pub scale_factor: Option<f32>,
}

fn cleanup_zeropoint(zp: &Arc<Tensor>) -> Option<Arc<Tensor>> {
    match zp.datum_type() {
        DatumType::U8 => cleanup_zeropoint_t::<u8>(zp),
        DatumType::I8 => cleanup_zeropoint_t::<i8>(zp),
        _ => Some(zp.clone()),
    }
}

fn cleanup_zeropoint_t<T: Datum + Zero + Copy>(zp: &Arc<Tensor>) -> Option<Arc<Tensor>> {
    let mut zp = zp.clone();
    if zp.rank() == 1 {
        let slice = zp.as_slice::<T>().unwrap();
        if slice[1..].iter().all(|&x| x == slice[0]) {
            zp = rctensor0(slice[0]);
        }
    }
    if zp.rank() == 0 && *zp.to_scalar::<T>().unwrap() == T::zero() {
        None
    } else {
        Some(zp.into_arc_tensor())
    }
}

impl QParams {
    pub fn new(dt: DatumType) -> QParams {
        QParams {
            c_datum_type: dt,
            zero_point_a: None,
            zero_point_b: None,
            zero_point_c: None,
            scale_factor: None,
        }
    }

    pub fn with_zero_point_a(self, zero_point: &Arc<Tensor>) -> QParams {
        QParams { zero_point_a: cleanup_zeropoint(zero_point), ..self }
    }

    pub fn with_zero_point_b(self, zero_point: &Arc<Tensor>) -> QParams {
        QParams { zero_point_b: cleanup_zeropoint(zero_point), ..self }
    }

    pub fn with_zero_point_c(self, zero_point: &Arc<Tensor>) -> QParams {
        QParams { zero_point_c: cleanup_zeropoint(zero_point), ..self }
    }

    pub fn with_scale_factor(self, scale_factor: f32) -> QParams {
        QParams { scale_factor: Some(scale_factor), ..self }
    }

    pub fn set_zero_point_a(&mut self, zero_point: &Arc<Tensor>) {
        self.zero_point_a = cleanup_zeropoint(zero_point);
    }

    pub fn set_zero_point_b(&mut self, zero_point: &Arc<Tensor>) {
        self.zero_point_b = cleanup_zeropoint(zero_point);
    }

    pub fn set_zero_point_c(&mut self, zero_point: &Arc<Tensor>) {
        self.zero_point_c = cleanup_zeropoint(zero_point);
    }

    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        self.scale_factor = Some(scale_factor)
    }
}

pub fn quantize_linear_f32_u8(x: f32, scale: f32, zero_point: i32) -> u8 {
    (((x * scale).round() as i32) + zero_point as i32)
        .max(u8::min_value() as i32)
        .min(u8::max_value() as i32) as u8
}

pub fn quantize_linear_f32_i8(x: f32, scale: f32, zero_point: i32) -> i8 {
    (((x * scale).round() as i32) + zero_point as i32)
        .max(i8::min_value() as i32)
        .min(i8::max_value() as i32) as i8
}

element_wise_oop!(quantize_linear_u8, QuantizeLinearU8 {scale: f32, zero_point: u8},
    [f32,i32] => u8 |op, xs, ys| {
        xs.iter().zip(ys.iter_mut()).for_each(|(x,y)|
            *y = quantize_linear_f32_u8(*x as f32, op.scale, op.zero_point as i32)
        );
        Ok(())
    }
);

element_wise_oop!(quantize_linear_i8, QuantizeLinearI8 {scale: f32, zero_point: i8},
    [f32,i32] => i8 |op, xs, ys| {
        xs.iter().zip(ys.iter_mut()).for_each(|(x,y)|
            *y = quantize_linear_f32_i8(*x as f32, op.scale, op.zero_point as i32)
        );
        Ok(())
    }
);

#[derive(Clone, Debug, new)]
pub struct DequantizeLinearF32 {
    scale: f32,
    zero_point: i32,
}

impl DequantizeLinearF32 {
    fn eval_t<T: Datum + AsPrimitive<i32>>(&self, input: &Tensor) -> TractResult<Tensor> {
        let mut output = unsafe { Tensor::uninitialized::<f32>(input.shape())? };
        input
            .as_slice::<T>()?
            .iter()
            .zip(output.as_slice_mut::<f32>()?.iter_mut())
            .for_each(|(x, y)| *y = (x.as_() - self.zero_point) as f32 * self.scale);
        Ok(output)
    }
}

impl Op for DequantizeLinearF32 {
    fn name(&self) -> Cow<str> {
        "DequantizeLinear".into()
    }

    fn validation(&self) -> Validation {
        Validation::Accurate
    }

    canonic!();
    op_as_typed_op!();
    op_as_pulsed_op!();
}

impl StatelessOp for DequantizeLinearF32 {
    fn eval(&self, inputs: TVec<Arc<Tensor>>) -> TractResult<TVec<Arc<Tensor>>> {
        let output = match inputs[0].datum_type() {
            DatumType::I8 => self.eval_t::<i8>(&inputs[0])?,
            DatumType::I32 => self.eval_t::<i32>(&inputs[0])?,
            DatumType::U8 => self.eval_t::<u8>(&inputs[0])?,
            dt => bail!("Unsupported type {:?}", dt),
        };
        Ok(tvec!(output.into_arc_tensor()))
    }
}

impl TypedOp for DequantizeLinearF32 {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        let mut fact = inputs[0].clone();
        fact.datum_type = f32::datum_type();
        Ok(tvec!(fact))
    }

    fn invariants(&self, model: &TypedModel, node: &TypedNode) -> TractResult<Invariants> {
        let a = model.outlet_fact(node.inputs[0])?;
        Ok((0..a.shape.rank()).into_iter().map(|axis| AxisInfo::simple(axis)).collect())
    }

    fn declutter(
        &self,
        model: &TypedModel,
        node: &TypedNode,
    ) -> TractResult<Option<TypedModelPatch>> {
        let mut current = node;
        while let Some(succ) = model.single_succ(current.id)? {
            let q_params = if let Some(op) = succ.op_as::<ElementWiseOp>() {
                if let Some(mop) = op.0.downcast_ref::<QuantizeLinearU8>() {
                    Some((mop.scale, mop.zero_point as i32, u8::datum_type()))
                } else if let Some(mop) = op.0.downcast_ref::<QuantizeLinearI8>() {
                    Some((mop.scale, mop.zero_point as i32, i8::datum_type()))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((scale, zero_point, dt)) = q_params {
                // first, try Op::quantize() on all ops in the chain
                let mut patch = TypedModelPatch::default();
                let mut wire: OutletId = patch.tap_model(model, node.inputs[0])?.into();
                let mut next = model.single_succ(node.id)?.unwrap();
                loop {
                    if let Some(op) = next
                        .op
                        .quantize(model, node, dt, scale, zero_point)
                        .chain_err(|| format!("Quantizing {}", next))?
                    {
                        wire = patch.wire_node(&*next.name, op, [wire].as_ref())?[0];
                    } else {
                        break;
                    }
                    if next.id == current.id {
                        patch.shunt_outside(OutletId::new(succ.id, 0), wire)?;
                        return Ok(Some(patch));
                    } else {
                        next = model.single_succ(next.id)?.unwrap();
                    }
                }
                // or else make a lookup table
                let mut adhoc_model = TypedModel::default();
                let mut wire = adhoc_model.add_source("ad-hoc", TypedFact::dt_shape(dt, [256].as_ref())?)?;
                let mut next = model.single_succ(node.id)?.unwrap();
                wire = adhoc_model.wire_node(&*node.name, node.op.clone(), [wire].as_ref())?[0];
                loop {
                    wire = adhoc_model.wire_node(&*node.name, next.op.clone(), [wire].as_ref())?[0];
                    if next.id == current.id {
                        break;
                    } else {
                        next = model.single_succ(next.id)?.unwrap();
                    }
                }
                wire = adhoc_model.wire_node(&*succ.name, succ.op.clone(), [wire].as_ref())?[0];
                adhoc_model.set_output_outlets(&[wire])?;
                let input = (0u8..=255).collect::<Vec<u8>>();
                let input = match dt {
                    DatumType::I8 => unsafe { tensor1(std::mem::transmute::<&[u8], &[i8]>(&*input)) },
                    DatumType::U8 => tensor1(&input),
                    _ => unreachable!(),
                };
                let output = SimplePlan::new(adhoc_model)?.run(tvec!(input))?.remove(0);
                let table:&[u8] = match dt {
                    DatumType::I8 => unsafe { std::mem::transmute(output.as_slice::<i8>()?) },
                    DatumType::U8 => output.as_slice::<u8>()?,
                    _ => unreachable!(),
                };
                let op = lookup_table((tract_linalg::ops().lut_u8)(table));
                let mut patch = TypedModelPatch::default();
                let mut wire: OutletId = patch.tap_model(model, node.inputs[0])?.into();
                wire = patch.wire_node(&*node.name, op, [wire].as_ref())?[0];
                patch.shunt_outside(OutletId::new(succ.id, 0), wire)?;
                return Ok(Some(patch));
            }
            let invariants = succ.op.invariants(model, succ)?;
            if invariants.element_wise() {
                current = succ;
            } else {
                break;
            }
        }
        Ok(None)
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
        target.wire_node(&*node.name, self.clone(), &[input])
    }

    typed_op_as_op!();
}

impl PulsedOp for DequantizeLinearF32 {
    fn pulsed_output_facts(&self, inputs: &[&PulsedFact]) -> TractResult<TVec<PulsedFact>> {
        let mut fact = inputs[0].clone();
        fact.datum_type = f32::datum_type();
        Ok(tvec!(fact))
    }

    pulsed_op_as_op!();
    pulsed_op_to_typed_op!();
}

element_wise_oop!(lookup_table, LookupTable {table: Box<dyn Lut>},
    [i8] => i8 |op, xs, ys| {
        ys.copy_from_slice(xs);
        unsafe {
            let casted = std::slice::from_raw_parts_mut(ys.as_mut_ptr() as *mut u8, ys.len());
            op.table.run(casted);
        }
        Ok(())
    },
    [u8] => u8 |op, xs, ys| {
        ys.copy_from_slice(xs);
        op.table.run(ys);
        Ok(())
    }
);
