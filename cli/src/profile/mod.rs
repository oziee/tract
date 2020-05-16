use std::collections::HashMap;
use std::fmt::{Debug, Display};

use ansi_term::Color::*;

use tract_core::internal::*;

use crate::display_graph::DisplayOptions;
use crate::errors::*;
use crate::format::*;
use crate::{Parameters, ProfilingMode};
use std::time::Duration;

mod regular;
//mod streaming;

trait Scalable {
    fn scale(self, scale: f32) -> Self;
}

impl Scalable for std::time::Duration {
    fn scale(self, scale: f32) -> Duration {
        Duration::from_secs_f32(scale * self.as_secs_f32())
    }
}

#[derive(Debug, Default)]
pub struct ProfileData {
    pub nodes: HashMap<TVec<usize>, Duration>,
}

impl ProfileData {
    pub fn add(&mut self, node_id: &[usize], dur: Duration) -> ::tract_core::TractResult<()> {
        *self.nodes.entry(node_id.into()).or_insert(Duration::default()) += dur;
        Ok(())
    }

    pub fn sub(&mut self, node_id: &[usize], dur: Duration) -> ::tract_core::TractResult<()> {
        *self.nodes.entry(node_id.into()).or_insert(Duration::default()) -= dur;
        Ok(())
    }

    pub fn nodes_above(&self, dur: Duration) -> CliResult<Vec<TVec<usize>>> {
        Ok(self.nodes.iter().filter(|n| *n.1 > dur).map(|n| n.0.clone()).collect())
    }

    fn op_name_for_id(model: &dyn Model, id: &[usize]) -> CliResult<String> {
        if id.len() == 1 {
            Ok(model.node_op(id[0]).name().into_owned())
        } else {
            let model = model.node_op(id[0]).as_typed().unwrap().nested_models()[0].1;
            Self::op_name_for_id(model, &id[1..])
        }
    }

    pub fn print_most_consuming_ops<F, O>(&self, model: &ModelImpl<F, O>) -> CliResult<()>
    where
        F: Fact + Clone + 'static + Hash,
        O: AsRef<dyn Op> + AsMut<dyn Op> + Display + Debug + Clone + 'static + Hash,
    {
        let sum = self.summed();
        println!("Most time consuming operations:");
        let mut operations = HashMap::new();
        let mut counters = HashMap::new();
        for (node, dur) in &self.nodes {
            let op_name = Self::op_name_for_id(model, node)?;
            *operations.entry(op_name.clone()).or_insert(Duration::default()) += *dur;
            *counters.entry(op_name).or_insert(0) += 1;
        }
        let mut operations: Vec<(&str, Duration)> =
            operations.iter().map(|(s, d)| (&**s, *d)).collect();
        operations.sort_by(|(_, a), (_, b)| b.cmp(&a));
        for (operation, measure) in operations.iter().take(5) {
            println!(
                "{:20} {:3} nodes: {}",
                Blue.bold().paint(*operation),
                counters[&**operation],
                dur_avg_oneline_ratio(*measure, sum)
            );
        }
        Ok(())
    }

    pub fn summed(&self) -> Duration {
        self.nodes.values().sum()
    }

    pub fn scale(&mut self, factor: f64) {
        self.nodes.values_mut().for_each(|n| *n = Duration::from_secs_f64(n.as_secs_f64() * factor))
    }
}

/// Handles the `profile` subcommand.
pub fn handle(
    params: &Parameters,
    profiling: ProfilingMode,
    display_options: DisplayOptions,
    monitor: Option<&readings_probe::Probe>,
) -> CliResult<()> {
    match &profiling {
        ProfilingMode::Regular { .. } => regular::handle(params, profiling, display_options),
        ProfilingMode::RegularBenching { .. } => {
            regular::handle_benching(params, profiling, monitor)
        }
    }
}
