// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use config::WorkerId;
use fastcrypto::hash::Hash;
use store::Store;
use tokio::{sync::watch, task::JoinHandle};
use types::{
    error::DagError,
    metered_channel::{Receiver, Sender},
    Batch, BatchDigest, ReconfigureNotification, WorkerPrimaryMessage,
};

#[cfg(test)]
#[path = "tests/processor_tests.rs"]
pub mod processor_tests;

/// Hashes and stores batches, it then outputs the batch's digest.
pub struct Processor;

impl Processor {
    #[must_use]
    pub fn spawn(
        // Our worker's id.
        id: WorkerId,
        // The persistent storage.
        store: Store<BatchDigest, Batch>,
        // Receive reconfiguration signals.
        mut rx_reconfigure: watch::Receiver<ReconfigureNotification>,
        // Input channel to receive batches.
        mut rx_batch: Receiver<Batch>,
        // Output channel to send out batches' digests.
        tx_digest: Sender<WorkerPrimaryMessage>,
        // Whether we are processing our own batches or the batches of other nodes.
        own_digest: bool,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(batch) = rx_batch.recv() => {
                        // Hash the batch.
                        let digest = batch.digest();

                        // Store the batch.
                        store.write(digest, batch).await;

                        // Deliver the batch's digest.
                        let message = match own_digest {
                            true => WorkerPrimaryMessage::OurBatch(digest, id),
                            false => WorkerPrimaryMessage::OthersBatch(digest, id),
                        };
                        if tx_digest
                            .send(message)
                            .await
                            .is_err() {
                            tracing::debug!("{}", DagError::ShuttingDown);
                        };
                    },

                    // Trigger reconfigure.
                    result = rx_reconfigure.changed() => {
                        result.expect("Committee channel dropped");
                        let message = rx_reconfigure.borrow().clone();
                        if let ReconfigureNotification::Shutdown = message {
                            return;
                        }
                    }
                }
            }
        })
    }
}
