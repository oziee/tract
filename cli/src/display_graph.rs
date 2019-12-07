use crate::CliResult;
use crate::SomeGraphDef;
use ansi_term::Color::*;
use ansi_term::Style;
use itertools::Itertools;
use std::borrow::Borrow;
use std::collections::HashMap;
#[allow(unused_imports)]
use std::convert::TryFrom;
use std::sync::Arc;
use tract_core::internal::*;
#[cfg(feature = "onnx")]
use tract_onnx::pb::ModelProto;
#[cfg(feature = "tf")]
use tract_tensorflow::tfpb::tensorflow::GraphDef;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplayOptions {
    pub konst: bool,
    pub quiet: bool,
    pub natural_order: bool,
    pub debug_op: bool,
    pub node_ids: Option<Vec<TVec<usize>>>,
    pub op_name: Option<String>,
    pub node_name: Option<String>,
    pub expect_canonic: bool,
    pub outlet_labels: bool,
    //    pub successors: Option<TVec<usize>>,
}

impl DisplayOptions {
    pub fn filter(
        &self,
        model: &dyn Model,
        current_prefix: &[usize],
        node_id: usize,
    ) -> CliResult<bool> {
        if let Some(nodes) = self.node_ids.as_ref() {
            return Ok(nodes.iter().any(|n| {
                n.len() == current_prefix.len() + 1
                    && &n[0..current_prefix.len()] == current_prefix
                    && *n.last().unwrap() == node_id
            }));
        }
        if let Some(node_name) = self.node_name.as_ref() {
            return Ok(model.node_name(node_id).starts_with(&*node_name));
        }
        if let Some(op_name) = self.op_name.as_ref() {
            return Ok(model.node_op(node_id).name().starts_with(op_name));
        }
        /*
        if let Some(successor) = self.successors {
            return Ok(model.node_inputs(node_id).iter().any(|i| i.node == successor));
        }
        */
        Ok(model.node_op(node_id).name() != "Const" || self.konst)
    }
}

#[derive(Debug, Clone)]
pub struct DisplayGraph<'a> {
    model: &'a dyn Model,
    prefix: TVec<usize>,
    pub options: Arc<DisplayOptions>,
    node_color: HashMap<usize, Style>,
    node_labels: HashMap<usize, Vec<String>>,
    node_sections: HashMap<usize, Vec<Vec<String>>>,
    node_nested_graphs: HashMap<usize, Vec<(String, DisplayGraph<'a>)>>,
}

impl<'a> DisplayGraph<'a> {
    pub fn render(&self) -> CliResult<()> {
        self.render_prefixed("")
    }

    pub fn render_prefixed(&self, prefix: &str) -> CliResult<()> {
        if self.options.quiet {
            return Ok(());
        }
        let node_ids = if self.options.natural_order {
            (0..self.model.nodes_len()).collect()
        } else {
            self.model.eval_order()?
        };
        for node in node_ids {
            if self.options.filter(self.model, &*self.prefix, node)? {
                self.render_node_prefixed(node, prefix)?
            }
        }
        Ok(())
    }

    pub fn render_node(&self, node_id: usize) -> CliResult<()> {
        self.render_node_prefixed(node_id, "")
    }

    pub fn render_node_prefixed(&self, node_id: usize, prefix: &str) -> CliResult<()> {
        let model = self.model.borrow();
        let name_color = self.node_color.get(&node_id).cloned().unwrap_or(White.into());
        let node_name = model.node_name(node_id);
        let node_op_name = model.node_op(node_id).name();
        println!(
            "{}{} {} {}",
            prefix,
            White.bold().paint(format!("{}", node_id)),
            (if node_name == "UnimplementedOp" {
                Red.bold()
            } else {
                if self.options.expect_canonic && !model.node_op(node_id).is_canonic() {
                    Yellow.bold()
                } else {
                    Blue.bold()
                }
            })
            .paint(node_op_name),
            name_color.italic().paint(node_name)
        );
        for label in self.node_labels.get(&node_id).unwrap_or(&vec![]).iter() {
            println!("{}  * {}", prefix, label);
        }
        if model.node_control_inputs(node_id).len() > 0 {
            println!(
                "{}  * control nodes: {}",
                prefix,
                model.node_control_inputs(node_id).iter().join(", ")
            );
        }
        for (ix, i) in model.node_inputs(node_id).iter().enumerate() {
            let star = if ix == 0 { '*' } else { ' ' };
            println!(
                "{}  {} input fact  #{}: {} {}",
                prefix,
                star,
                ix,
                White.bold().paint(format!("{:?}", i)),
                model.outlet_fact_format(*i),
            );
        }
        for ix in 0..model.node_output_count(node_id) {
            let star = if ix == 0 { '*' } else { ' ' };
            let io = if let Some(id) = self
                .model
                .borrow()
                .input_outlets()
                .iter()
                .position(|n| n.node == node_id && n.slot == ix)
            {
                Cyan.bold().paint(format!("MODEL INPUT #{}", id)).to_string()
            } else if let Some(id) = self
                .model
                .borrow()
                .output_outlets()
                .iter()
                .position(|n| n.node == node_id && n.slot == ix)
            {
                Yellow.bold().paint(format!("MODEL OUTPUT #{}", id)).to_string()
            } else {
                "".to_string()
            };
            let outlet = OutletId::new(node_id, ix);
            let successors = model.outlet_successors(outlet);
            println!(
                "{}  {} output fact #{}: {} {} {}",
                prefix,
                star,
                ix,
                model.outlet_fact_format(outlet),
                White.bold().paint(successors.iter().map(|s| format!("{:?}", s)).join(" ")),
                io
            );
            if self.options.outlet_labels {
                if let Some(label) = model.outlet_label(OutletId::new(node_id, ix)) {
                    println!("{}            {} ", prefix, White.italic().paint(label));
                }
            }
        }
        for info in model.node_op(node_id).info()? {
            println!("{}  * {}", prefix, info);
        }
        if self.options.debug_op {
            println!("{}  * {:?}", prefix, model.node_op(node_id));
        }
        if let Some(node_sections) = self.node_sections.get(&node_id) {
            for section in node_sections {
                if section.is_empty() {
                    continue;
                }
                println!("{}  * {}", prefix, section[0]);
                for s in &section[1..] {
                    println!("{}    {}", prefix, s);
                }
            }
        }
        for (label, sub) in self.node_nested_graphs.get(&node_id).unwrap_or(&vec![]) {
            sub.render_prefixed(&format!(" {}{}.{} >> ", prefix, model.node_name(node_id), label))?
        }
        Ok(())
    }

    pub fn from_model_and_options(
        model: &'a dyn Model,
        options: Arc<DisplayOptions>,
    ) -> CliResult<DisplayGraph<'a>> {
        Self::from_model_prefix_and_options(model, [].as_ref(), options)
    }

    fn from_model_prefix_and_options(
        model: &'a dyn Model,
        prefix: &[usize],
        options: Arc<DisplayOptions>,
    ) -> CliResult<DisplayGraph<'a>> {
        let mut node_nested_graphs = HashMap::new();
        for n in 0..model.nodes_len() {
            let subs = model.node_op(n).nested_models();
            if subs.len() > 0 {
                let mut prefix: TVec<usize> = prefix.into();
                prefix.push(n);
                node_nested_graphs.insert(
                    n,
                    subs.into_iter()
                        .map(|(label, sub)| {
                            Ok((
                                label.into_owned(),
                                Self::from_model_prefix_and_options(
                                    sub,
                                    &*prefix,
                                    Arc::clone(&options),
                                )?,
                            ))
                        })
                        .collect::<CliResult<_>>()?,
                );
            }
        }
        Ok(DisplayGraph {
            model,
            prefix: prefix.into(),
            options,
            node_color: HashMap::new(),
            node_labels: HashMap::new(),
            node_sections: HashMap::new(),
            node_nested_graphs,
        })
    }

    pub fn with_graph_def(self, graph_def: &SomeGraphDef) -> CliResult<DisplayGraph<'a>> {
        match graph_def {
            SomeGraphDef::NoGraphDef => Ok(self),
            #[cfg(feature = "kaldi")]
            SomeGraphDef::Kaldi(kaldi) => self.with_kaldi(kaldi),
            #[cfg(feature = "onnx")]
            SomeGraphDef::Onnx(onnx, _) => self.with_onnx_model(onnx),
            #[cfg(feature = "tf")]
            SomeGraphDef::Tf(tf) => self.with_tf_graph_def(tf),
        }
    }

    pub fn set_node_color<S: Into<Style>>(&mut self, id: usize, color: S) -> CliResult<()> {
        self.node_color.insert(id, color.into());
        Ok(())
    }

    pub fn add_node_label<S: Into<String>>(&mut self, id: &[usize], label: S) -> CliResult<()> {
        if id.len() == 1 {
            self.node_labels.entry(id[0]).or_insert(vec![]).push(label.into());
            Ok(())
        } else {
            self.node_nested_graphs.get_mut(&id[0]).unwrap()[0].1.add_node_label(&id[1..], label)
        }
    }

    pub fn add_node_section(&mut self, id: &[usize], section: Vec<String>) -> CliResult<()> {
        if id.len() == 1 {
            self.node_sections.entry(id[0]).or_insert(vec![]).push(section);
            Ok(())
        } else {
            self.node_nested_graphs.get_mut(&id[0]).unwrap()[0]
                .1
                .add_node_section(&id[1..], section)
        }
    }

    #[cfg(feature = "kaldi")]
    pub fn with_kaldi(
        mut self,
        proto_model: &tract_kaldi::KaldiProtoModel,
    ) -> CliResult<DisplayGraph<'a>> {
        use tract_kaldi::model::NodeLine;
        let bold = Style::new().bold();
        for (name, proto_node) in &proto_model.config_lines.nodes {
            if let Ok(node_id) = self.model.borrow().node_id_by_name(&*name) {
                let mut vs = vec![];
                match proto_node {
                    NodeLine::Component(compo) => {
                        let comp = &proto_model.components[&compo.component];
                        for (k, v) in &comp.attributes {
                            let value = format!("{:?}", v);
                            vs.push(format!("Attr {}: {:.240}", bold.paint(k), value));
                        }
                    }
                    _ => (),
                }
                self.add_node_section(&[node_id], vs)?;
            }
        }
        Ok(self)
    }

    #[cfg(feature = "tf")]
    pub fn with_tf_graph_def(mut self, graph_def: &GraphDef) -> CliResult<DisplayGraph<'a>> {
        let bold = Style::new().bold();
        for gnode in graph_def.node.iter() {
            if let Ok(node_id) = self.model.borrow().node_id_by_name(&gnode.name) {
                let mut v = vec![];
                for a in gnode.attr.iter() {
                    let value = if let Some(
                        tract_tensorflow::tfpb::tensorflow::attr_value::Value::Tensor(r),
                    ) = &a.1.value
                    {
                        format!("{:?}", r)
                    } else {
                        format!("{:?}", a.1)
                    };
                    v.push(format!("Attr {}: {:.240}", bold.paint(a.0), value));
                }
                self.add_node_section(&[node_id], v)?;
            }
        }
        Ok(self)
    }

    #[cfg(feature = "onnx")]
    pub fn with_onnx_model(mut self, model_proto: &ModelProto) -> CliResult<DisplayGraph<'a>> {
        let bold = Style::new().bold();
        for gnode in model_proto.graph.as_ref().unwrap().node.iter() {
            let mut node_name = &gnode.name;
            if node_name == "" && gnode.output.len() > 0 {
                node_name = &gnode.output[0];
            }
            if let Ok(id) = self.model.borrow().node_id_by_name(&*node_name) {
                let mut v = vec![];
                for a in gnode.attribute.iter() {
                    let value = if let Some(t) =  &a.t {
                        format!("{:?}", Tensor::try_from(t)?)
                    } else {
                        format!("{:?}", a)
                    };
                    v.push(format!("Attr {}: {:.240}", bold.paint(&a.name), value));
                }
                self.add_node_section(&[id], v)?;
            }
        }
        Ok(self)
    }
}
