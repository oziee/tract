use crate::internal::*;
use crate::model::{Fact, ModelImpl, OutletId};
use std::collections::HashMap;
use std::convert::*;
use std::fmt;

pub trait Translate<TI1, O1, TI2, O2>: fmt::Debug
where
    TI1: Fact + Hash + Clone + 'static,
    TI2: Fact + Hash + Clone + 'static,
    O1: fmt::Display + fmt::Debug + AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static + Hash,
    O2: fmt::Display + fmt::Debug + AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static + Hash,
{
    fn translate_node(
        &self,
        source: &ModelImpl<TI1, O1>,
        node: &BaseNode<TI1, O1>,
        target: &mut ModelImpl<TI2, O2>,
        mapping: &HashMap<OutletId, OutletId>,
    ) -> TractResult<TVec<OutletId>>;

    fn translate_model(&self, source: &ModelImpl<TI1, O1>) -> TractResult<ModelImpl<TI2, O2>> {
        Ok(self.translate_model_with_mappings(source)?.0)
    }

    fn translate_model_with_mappings(
        &self,
        source: &ModelImpl<TI1, O1>,
    ) -> TractResult<(ModelImpl<TI2, O2>, HashMap<OutletId, OutletId>)> {
        let mut target = ModelImpl::default();
        let mut mapping = HashMap::new();
        for old_id in source.eval_order()? {
            let node = source.node(old_id);
            debug!("Translating {} {:?}", node, self);
            let outlets = self
                .translate_node(&source, node, &mut target, &mapping)
                .chain_err(|| format!("Translating node {} {:?}", node, self))?;
            for (ix, outlet) in outlets.into_iter().enumerate() {
                mapping.insert(OutletId::new(node.id, ix), outlet);
                if let Some(label) = source.outlet_label(OutletId::new(node.id, ix)) {
                    target.set_outlet_label(outlet, label.to_string());
                }
            }
        }
        // do not drop inputs, even if they are useless, to maintain interface
        for i in source.input_outlets()? {
            if !mapping.contains_key(i) {
                let node = source.node(i.node);
                debug!("Translate useless source {}", node);
                let outlets = self
                    .translate_node(&source, node, &mut target, &mapping)
                    .chain_err(|| format!("Translating input {} {:?}", node, self))?;
                mapping.insert(*i, outlets[0]);
            }
        }
        // maintaining order of i/o interface
        target.inputs = source.input_outlets()?.iter().map(|i| mapping[&i]).collect();
        target.outputs = source.output_outlets()?.iter().map(|o| mapping[&o]).collect();
        Ok((target, mapping))
    }
}

#[derive(Debug)]
pub struct IntoTranslator;
impl<TI1, O1, TI2, O2, EO, ETI> Translate<TI1, O1, TI2, O2> for IntoTranslator
where
    TractError: From<EO> + From<ETI>,
    TI1: Fact + Hash + Clone + 'static,
    TI2: Fact + Hash + for<'a> TryFrom<&'a TI1, Error = EO> + Clone + 'static,
    O1: fmt::Display + fmt::Debug + Clone + AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static + Hash,
    O2: fmt::Display
        + for<'a> TryFrom<&'a O1, Error = ETI>
        + fmt::Debug
        + AsRef<dyn Op>
        + AsMut<dyn Op>
        + Clone
        + Hash
        + 'static,
{
    fn translate_node(
        &self,
        _source: &ModelImpl<TI1, O1>,
        node: &BaseNode<TI1, O1>,
        target: &mut ModelImpl<TI2, O2>,
        mapping: &HashMap<OutletId, OutletId>,
    ) -> TractResult<TVec<OutletId>> {
        let new_op = O2::try_from(&node.op)?;
        let facts = node
            .outputs
            .iter()
            .map(|of| Ok(TI2::try_from(&of.fact)?))
            .collect::<TractResult<TVec<_>>>()?;
        let new_id = target.add_node(node.name.clone(), new_op, facts)?;
        for (ix, o) in node.inputs.iter().enumerate() {
            target.add_edge(mapping[o], InletId::new(new_id, ix))?
        }
        Ok(node.outputs.iter().enumerate().map(|(ix, _)| OutletId::new(new_id, ix)).collect())
    }
}
