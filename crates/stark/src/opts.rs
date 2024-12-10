use std::env;

use serde::{Deserialize, Serialize};
use sysinfo::System;

const MAX_SHARD_SIZE: usize = 1 << 21;
const RECURSION_MAX_SHARD_SIZE: usize = 1 << 22;
const MAX_SHARD_BATCH_SIZE: usize = 8;
const DEFAULT_TRACE_GEN_WORKERS: usize = 1;
const DEFAULT_CHECKPOINTS_CHANNEL_CAPACITY: usize = 128;
const DEFAULT_RECORDS_AND_TRACES_CHANNEL_CAPACITY: usize = 1;
const MAX_DEFERRED_SPLIT_THRESHOLD: usize = 1 << 15;

/// Options to configure the SP1 prover for core and recursive proofs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SP1ProverOpts {
    /// Options for the core prover.
    pub core_opts: SP1CoreOpts,
    /// Options for the recursion prover.
    pub recursion_opts: SP1CoreOpts,
}

impl SP1ProverOpts {
    /// Get the default prover options for a prover on CPU.
    #[must_use]
    pub fn cpu() -> Self {
        Self { core_opts: SP1CoreOpts::default(), recursion_opts: SP1CoreOpts::recursion() }
    }

    /// Get the default prover options for a prover on GPU given the amount of CPU and GPU memory.
    #[must_use]
    pub fn gpu(cpu_ram_gb: usize, gpu_ram_gb: usize) -> Self {
        let mut opts = SP1ProverOpts::cpu();

        // Set the core options.
        if 24 <= gpu_ram_gb {
            let log2_shard_size = 21;
            opts.core_opts.shard_size = 1 << log2_shard_size;
            opts.core_opts.shard_batch_size = 1;

            let log2_deferred_threshold = 14;
            opts.core_opts.split_opts = SplitOpts::new(1 << log2_deferred_threshold);

            opts.core_opts.records_and_traces_channel_capacity = 4;
            opts.core_opts.trace_gen_workers = 4;

            if cpu_ram_gb <= 20 {
                opts.core_opts.records_and_traces_channel_capacity = 1;
                opts.core_opts.trace_gen_workers = 2;
            }
        } else {
            unreachable!("not enough gpu memory");
        }

        // Set the recursion options.
        opts.recursion_opts.shard_batch_size = 1;

        opts
    }
}

/// Options for the core prover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SP1CoreOpts {
    /// The size of a shard in terms of cycles.
    pub shard_size: usize,
    /// The size of a batch of shards in terms of cycles.
    pub shard_batch_size: usize,
    /// Options for splitting deferred events.
    pub split_opts: SplitOpts,
    /// The number of workers to use for generating traces.
    pub trace_gen_workers: usize,
    /// The capacity of the channel for checkpoints.
    pub checkpoints_channel_capacity: usize,
    /// The capacity of the channel for records and traces.
    pub records_and_traces_channel_capacity: usize,
}

/// Calculate the default shard size using an empirically determined formula.
///
/// For super memory constrained machines, we need to set shard size to 2^18.
/// Otherwise, we use a linear formula derived from experimental results.
/// The data comes from benchmarking the maximum physical memory usage
/// of [rsp](https://github.com/succinctlabs/rsp) on a variety of shard sizes and
/// shard batch sizes, and performing linear regression on the results.
#[allow(clippy::cast_precision_loss)]
fn shard_size(total_available_mem: u64) -> usize {
    let log_shard_size = match total_available_mem {
        0..=14 => 17,
        m => (((m as f64).log2() * 0.619) + 16.2).floor() as usize,
    };
    std::cmp::min(1 << log_shard_size, MAX_SHARD_SIZE)
}

/// Calculate the default shard batch size using an empirically determined formula.
///
/// For memory constrained machines, we need to set shard batch size to either 1 or 2.
/// For machines with a very large amount of memory, we can use batch size 8. Empirically,
/// going above 8 doesn't result in a significant speedup.
/// For most machines, we can just use batch size 4.
fn shard_batch_size(total_available_mem: u64) -> usize {
    match total_available_mem {
        0..=16 => 1,
        17..=48 => 2,
        256.. => MAX_SHARD_BATCH_SIZE,
        _ => 4,
    }
}

impl Default for SP1CoreOpts {
    fn default() -> Self {
        let split_threshold = env::var("SPLIT_THRESHOLD")
            .map(|s| s.parse::<usize>().unwrap_or(MAX_DEFERRED_SPLIT_THRESHOLD))
            .unwrap_or(MAX_DEFERRED_SPLIT_THRESHOLD)
            .max(MAX_DEFERRED_SPLIT_THRESHOLD);

        let sys = System::new_all();
        let total_available_mem = sys.total_memory() / (1024 * 1024 * 1024);
        let default_shard_size = shard_size(total_available_mem);
        let default_shard_batch_size = shard_batch_size(total_available_mem);

        let shard_size = env::var("SHARD_SIZE").map_or_else(
            |_| default_shard_size,
            |s| s.parse::<usize>().unwrap_or(default_shard_size),
        );

        Self {
            shard_size,
            shard_batch_size: env::var("SHARD_BATCH_SIZE").map_or_else(
                |_| default_shard_batch_size,
                |s| s.parse::<usize>().unwrap_or(default_shard_batch_size),
            ),
            split_opts: SplitOpts::new(split_threshold),
            trace_gen_workers: env::var("TRACE_GEN_WORKERS").map_or_else(
                |_| DEFAULT_TRACE_GEN_WORKERS,
                |s| s.parse::<usize>().unwrap_or(DEFAULT_TRACE_GEN_WORKERS),
            ),
            checkpoints_channel_capacity: env::var("CHECKPOINTS_CHANNEL_CAPACITY").map_or_else(
                |_| DEFAULT_CHECKPOINTS_CHANNEL_CAPACITY,
                |s| s.parse::<usize>().unwrap_or(DEFAULT_CHECKPOINTS_CHANNEL_CAPACITY),
            ),
            records_and_traces_channel_capacity: env::var("RECORDS_AND_TRACES_CHANNEL_CAPACITY")
                .map_or_else(
                    |_| DEFAULT_RECORDS_AND_TRACES_CHANNEL_CAPACITY,
                    |s| s.parse::<usize>().unwrap_or(DEFAULT_RECORDS_AND_TRACES_CHANNEL_CAPACITY),
                ),
        }
    }
}

impl SP1CoreOpts {
    /// Get the default options for the recursion prover.
    #[must_use]
    pub fn recursion() -> Self {
        let mut opts = Self::default();
        opts.shard_size = RECURSION_MAX_SHARD_SIZE;
        opts
    }
}

/// Options for splitting deferred events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitOpts {
    /// The threshold for combining the memory init/finalize events in to the current shard in
    /// terms of cycles.
    pub combine_memory_threshold: usize,
    /// The threshold for default events.
    pub deferred: usize,
    /// The threshold for keccak events.
    pub keccak: usize,
    /// The threshold for sha extend events.
    pub sha_extend: usize,
    /// The threshold for sha compress events.
    pub sha_compress: usize,
    /// The threshold for memory events.
    pub memory: usize,
}

impl SplitOpts {
    /// Create a new [`SplitOpts`] with the given threshold.
    ///
    /// The constants here need to be chosen very carefully to prevent OOM. Consult @jtguibas on
    /// how to change them.
    #[must_use]
    pub fn new(deferred_split_threshold: usize) -> Self {
        Self {
            combine_memory_threshold: 1 << 26,
            deferred: deferred_split_threshold,
            keccak: 8 * deferred_split_threshold / 24,
            sha_extend: 32 * deferred_split_threshold / 48,
            sha_compress: 32 * deferred_split_threshold / 80,
            memory: 64 * deferred_split_threshold,
        }
    }
}
