// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! An interactive dataflow server.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::anyhow;
use crossbeam_channel::TryRecvError;
use timely::communication::initialize::WorkerGuards;
use timely::communication::Allocate;
use timely::worker::Worker as TimelyWorker;
use tokio::sync::mpsc;

use mz_dataflow_types::client::{ComputeCommand, ComputeResponse, LocalClient, LocalComputeClient};
use mz_dataflow_types::sources::AwsExternalId;
use mz_ore::metrics::MetricsRegistry;
use mz_ore::now::NowFn;
use mz_storage::boundary::ComputeReplay;

use crate::compute_state::ActiveComputeState;
use crate::compute_state::ComputeState;
use crate::SinkBaseMetrics;
use crate::{TraceManager, TraceMetrics};

/// Configures a dataflow server.
pub struct Config {
    /// The number of worker threads to spawn.
    pub workers: usize,
    /// The Timely configuration
    pub timely_config: timely::Config,
    /// Whether the server is running in experimental mode.
    pub experimental_mode: bool,
    /// Function to get wall time now.
    pub now: NowFn,
    /// Metrics registry through which dataflow metrics will be reported.
    pub metrics_registry: MetricsRegistry,
    /// An external ID to use for all AWS AssumeRole operations.
    pub aws_external_id: AwsExternalId,
}

/// A handle to a running dataflow server.
///
/// Dropping this object will block until the dataflow computation ceases.
pub struct Server {
    _worker_guards: WorkerGuards<()>,
}

/// Initiates a timely dataflow computation, processing materialized commands.
///
/// * `create_boundary`: A function to obtain the worker-local boundary components.
pub fn serve_boundary<CR: ComputeReplay, B: Fn(usize) -> CR + Send + Sync + 'static>(
    config: Config,
    create_boundary: B,
) -> Result<(Server, LocalComputeClient), anyhow::Error> {
    assert!(config.workers > 0);

    // Various metrics related things.
    let sink_metrics = SinkBaseMetrics::register_with(&config.metrics_registry);
    let trace_metrics = TraceMetrics::register_with(&config.metrics_registry);
    // Bundle metrics to conceal complexity.
    let metrics_bundle = (sink_metrics, trace_metrics);

    // Construct endpoints for each thread that will receive the coordinator's
    // sequenced command stream and send the responses to the coordinator.
    //
    // TODO(benesch): package up this idiom of handing out ownership of N items
    // to the N timely threads that will be spawned. The Mutex<Vec<Option<T>>>
    // is hard to read through.
    let (command_txs, command_rxs): (Vec<_>, Vec<_>) = (0..config.workers)
        .map(|_| crossbeam_channel::unbounded())
        .unzip();
    let (compute_response_txs, compute_response_rxs): (Vec<_>, Vec<_>) = (0..config.workers)
        .map(|_| mpsc::unbounded_channel())
        .unzip();
    // Mutexes around a vector of optional (take-able) pairs of (tx, rx) for worker/client communication.
    let command_channels: Mutex<Vec<_>> = Mutex::new(command_rxs.into_iter().map(Some).collect());
    let compute_response_channels: Mutex<Vec<_>> =
        Mutex::new(compute_response_txs.into_iter().map(Some).collect());

    let tokio_executor = tokio::runtime::Handle::current();

    let worker_guards = timely::execute::execute(config.timely_config, move |timely_worker| {
        let timely_worker_index = timely_worker.index();
        let compute_boundary = create_boundary(timely_worker_index);
        let _tokio_guard = tokio_executor.enter();
        let command_rx = command_channels.lock().unwrap()[timely_worker_index % config.workers]
            .take()
            .unwrap();
        let compute_response_tx = compute_response_channels.lock().unwrap()
            [timely_worker_index % config.workers]
            .take()
            .unwrap();
        let (_sink_metrics, _trace_metrics) = metrics_bundle.clone();
        Worker {
            timely_worker,
            command_rx,
            compute_state: None,
            compute_boundary,
            compute_response_tx,
            metrics_bundle: metrics_bundle.clone(),
        }
        .run()
    })
    .map_err(|e| anyhow!("{}", e))?;
    let worker_threads = worker_guards
        .guards()
        .iter()
        .map(|g| g.thread().clone())
        .collect::<Vec<_>>();
    let compute_client = LocalClient::new(compute_response_rxs, command_txs, worker_threads);
    let server = Server {
        _worker_guards: worker_guards,
    };
    Ok((server, compute_client))
}

/// State maintained for each worker thread.
///
/// Much of this state can be viewed as local variables for the worker thread,
/// holding state that persists across function calls.
struct Worker<'w, A, CR>
where
    A: Allocate,
    CR: ComputeReplay,
{
    /// The underlying Timely worker.
    timely_worker: &'w mut TimelyWorker<A>,
    /// The channel from which commands are drawn.
    command_rx: crossbeam_channel::Receiver<ComputeCommand>,
    /// The state associated with rendering dataflows.
    compute_state: Option<ComputeState>,
    /// The boundary between storage and compute layers, compute side.
    compute_boundary: CR,
    /// The channel over which compute responses are reported.
    compute_response_tx: mpsc::UnboundedSender<ComputeResponse>,
    /// Metrics bundle.
    metrics_bundle: (SinkBaseMetrics, TraceMetrics),
}

impl<'w, A, CR> Worker<'w, A, CR>
where
    A: Allocate + 'w,
    CR: ComputeReplay,
{
    /// Draws from `dataflow_command_receiver` until shutdown.
    fn run(&mut self) {
        let mut shutdown = false;
        while !shutdown {
            // Enable trace compaction.
            if let Some(compute_state) = &mut self.compute_state {
                compute_state.traces.maintenance();
            }

            // Ask Timely to execute a unit of work. If Timely decides there's
            // nothing to do, it will park the thread. We rely on another thread
            // unparking us when there's new work to be done, e.g., when sending
            // a command or when new Kafka messages have arrived.
            self.timely_worker.step_or_park(None);

            // Report frontier information back the coordinator.
            if let Some(mut compute_state) = self.activate_compute() {
                compute_state.report_compute_frontiers();
            }

            // Handle any received commands.
            let mut cmds = vec![];
            let mut empty = false;
            while !empty {
                match self.command_rx.try_recv() {
                    Ok(cmd) => cmds.push(cmd),
                    Err(TryRecvError::Empty) => empty = true,
                    Err(TryRecvError::Disconnected) => {
                        empty = true;
                        shutdown = true;
                    }
                }
            }
            for cmd in cmds {
                let mut should_drop_compute = false;
                match &cmd {
                    ComputeCommand::CreateInstance(_logging) => {
                        self.compute_state = Some(ComputeState {
                            traces: TraceManager::new(
                                self.metrics_bundle.1.clone(),
                                self.timely_worker.index(),
                            ),
                            dataflow_tokens: HashMap::new(),
                            tail_response_buffer: std::rc::Rc::new(std::cell::RefCell::new(
                                Vec::new(),
                            )),
                            sink_write_frontiers: HashMap::new(),
                            pending_peeks: Vec::new(),
                            reported_frontiers: HashMap::new(),
                            sink_metrics: self.metrics_bundle.0.clone(),
                            materialized_logger: None,
                        });
                    }
                    ComputeCommand::DropInstance => {
                        should_drop_compute = true;
                    }
                    _ => (),
                }

                self.activate_compute().unwrap().handle_compute_command(cmd);

                if should_drop_compute {
                    self.compute_state = None;
                }
            }

            if let Some(mut compute_state) = self.activate_compute() {
                compute_state.process_peeks();
                compute_state.process_tails();
            }
        }
        if let Some(mut compute_state) = self.activate_compute() {
            compute_state.compute_state.traces.del_all_traces();
            compute_state.shutdown_logging();
        }
    }

    fn activate_compute(&mut self) -> Option<ActiveComputeState<A, CR>> {
        if let Some(compute_state) = &mut self.compute_state {
            Some(ActiveComputeState {
                timely_worker: &mut *self.timely_worker,
                compute_state,
                response_tx: &mut self.compute_response_tx,
                boundary: &mut self.compute_boundary,
            })
        } else {
            None
        }
    }
}
