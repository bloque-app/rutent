//! The state machine on its host, as the assembly feeds it (`T-024-02`): every input the log takes, in the log's
//! order, is submitted to a host of the host contract running `rutent-stm`'s `Stm`.
//!
//! [`Machine`] is the host as the assembly drives it, whatever the host is; [`Environment`] is where one runs, and
//! makes them. The reference host ([`Reference`]) is the one the root workspace runs; `rutent-scenarios-vos`, in the
//! VOS client's workspace, adds VOS on three replicas, so no VOS crate enters the root lock (`DEC-26`).
//!
//! [`Feed`] holds the machine and what it was fed. A delivery fault ([`Delivery`]) changes how the inputs reach it:
//! each twice, or those of different orders interleaved by the run's seed, each order's own kept in the log's
//! order. [`Feed::swap`] moves the machine to another environment mid-run: halted, exported, imported into a fresh
//! host there, and resumed (`REQ-OPS-3`, `NFR-6`).

use rutent_contracts::state_machine::host::{
    Delivery as Delivered, Host, HostError, SnapshotEntry, TransitionRecord,
};
use rutent_contracts::state_machine::{Id, Order, RecordedInput};
use rutent_stm::Stm;
use rutent_stm::reference_host::{MemoryStore, ReferenceHost};

use crate::kit::Seeded;

/// A host running `Stm`, as the assembly drives it.
pub trait Machine {
    /// Records and applies one input.
    fn submit(&mut self, input: RecordedInput) -> Result<Delivered, HostError>;
    /// Every order with a record.
    fn orders(&self) -> Result<Vec<Id>, HostError>;
    /// An order's current state.
    fn state(&self, order: &Id) -> Result<Option<Order>, HostError>;
    /// An order's journal.
    fn journal(&self, order: &Id) -> Result<Vec<TransitionRecord>, HostError>;
    /// Stops taking submits, durably.
    fn halt(&mut self) -> Result<(), HostError>;
    /// Takes submits again.
    fn resume(&mut self) -> Result<(), HostError>;
    /// Everything it holds, as a snapshot stream's entries; only once halted.
    fn export(&self) -> Result<Vec<SnapshotEntry>, HostError>;
}

/// Any host of the host contract running `Stm`, as a [`Machine`].
pub struct Hosted<H>(pub H);

impl<H: Host<Stm>> Machine for Hosted<H> {
    fn submit(&mut self, input: RecordedInput) -> Result<Delivered, HostError> {
        self.0.submit(input)
    }

    fn orders(&self) -> Result<Vec<Id>, HostError> {
        self.0.orders()
    }

    fn state(&self, order: &Id) -> Result<Option<Order>, HostError> {
        self.0.state(order)
    }

    fn journal(&self, order: &Id) -> Result<Vec<TransitionRecord>, HostError> {
        self.0.journal(order)
    }

    fn halt(&mut self) -> Result<(), HostError> {
        self.0.halt()
    }

    fn resume(&mut self) -> Result<(), HostError> {
        self.0.resume()
    }

    fn export(&self) -> Result<Vec<SnapshotEntry>, HostError> {
        self.0.export()?.collect()
    }
}

/// Where a machine runs.
pub trait Environment {
    /// Its name, for the run's notes.
    fn name(&self) -> String;
    /// A fresh host on an empty store.
    fn open(&mut self) -> Result<Box<dyn Machine>, HostError>;
    /// A fresh host holding exactly what `entries` hold, halted, as an import leaves it.
    fn import(&mut self, entries: Vec<SnapshotEntry>) -> Result<Box<dyn Machine>, HostError>;
}

/// The in-memory reference host (`T-005-04`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Reference;

impl Environment for Reference {
    fn name(&self) -> String {
        "the reference host".to_owned()
    }

    fn open(&mut self) -> Result<Box<dyn Machine>, HostError> {
        Ok(Box::new(Hosted(ReferenceHost::<Stm>::open(
            MemoryStore::new(),
        )?)))
    }

    fn import(&mut self, entries: Vec<SnapshotEntry>) -> Result<Box<dyn Machine>, HostError> {
        let stream = Box::new(entries.into_iter().map(Ok));
        Ok(Box::new(Hosted(ReferenceHost::<Stm>::import(
            MemoryStore::new(),
            stream,
        )?)))
    }
}

/// How the log's inputs reach the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delivery {
    /// Each once, in the log's order.
    #[default]
    Once,
    /// Each twice in a row: a repeated delivery, which the host records once (`exactly-once`).
    Repeated,
    /// Held back until a scenario's step ends, and then those of different orders interleaved by the run's seed,
    /// each order's kept in the log's order (`per-order-sequence`).
    Reordered,
}

/// The machine, and how far into the log it has been fed.
pub struct Feed {
    machine: Box<dyn Machine>,
    environment: String,
    fed: usize,
    delivery: Delivery,
    shuffle: Seeded,
    /// Every submit the machine answered, in order: each input as delivered and whether it was new.
    pub deliveries: Vec<(RecordedInput, bool)>,
    /// Every environment the machine ran in, in order.
    pub environments: Vec<String>,
}

impl Feed {
    /// A fresh machine in `environment`, its inputs delivered as `delivery` says, interleaved by `seed`.
    pub fn new(
        environment: &mut dyn Environment,
        delivery: Delivery,
        seed: u64,
    ) -> Result<Self, HostError> {
        Ok(Self {
            machine: environment.open()?,
            environment: environment.name(),
            fed: 0,
            delivery,
            shuffle: Seeded::new(seed ^ 0x005e_ed0f_de11_7e57),
            deliveries: Vec::new(),
            environments: vec![environment.name()],
        })
    }

    /// The machine.
    pub fn machine(&self) -> &dyn Machine {
        &*self.machine
    }

    /// The environment it runs in now.
    pub fn environment(&self) -> &str {
        &self.environment
    }

    /// From now on, inputs reach the machine as `delivery` says.
    pub fn deliver(&mut self, delivery: Delivery) {
        self.delivery = delivery;
    }

    /// Feeds the machine the inputs of `log` from where it last stopped: `log` is every input the log took, in its
    /// order. Under [`Delivery::Reordered`] they wait for [`Feed::flush`].
    pub fn feed(&mut self, log: &[RecordedInput]) -> Result<(), HostError> {
        if self.delivery == Delivery::Reordered {
            return Ok(());
        }
        self.flush(log)
    }

    /// Feeds the machine every input of `log` it hasn't had, held back or not.
    pub fn flush(&mut self, log: &[RecordedInput]) -> Result<(), HostError> {
        let fresh: Vec<RecordedInput> = log.iter().skip(self.fed).cloned().collect();
        let order = match self.delivery {
            Delivery::Reordered => self.interleave(fresh),
            _ => fresh,
        };
        for input in order {
            let times = if self.delivery == Delivery::Repeated {
                2
            } else {
                1
            };
            for _ in 0..times {
                let answer = self.machine.submit(input.clone())?;
                self.deliveries
                    .push((input.clone(), matches!(answer, Delivered::Recorded(_))));
            }
            self.fed += 1;
        }
        Ok(())
    }

    /// `inputs`, those of different orders interleaved by the seed, each order's in the order given.
    fn interleave(&mut self, inputs: Vec<RecordedInput>) -> Vec<RecordedInput> {
        let mut queues: Vec<(String, std::collections::VecDeque<RecordedInput>)> = Vec::new();
        for input in inputs {
            let order = input.subject.order.as_str().to_owned();
            match queues.iter_mut().find(|(o, _)| *o == order) {
                Some((_, q)) => q.push_back(input),
                None => queues.push((order, std::collections::VecDeque::from([input]))),
            }
        }
        let mut out = Vec::new();
        while !queues.is_empty() {
            let at = usize::try_from(self.shuffle.below(queues.len() as u64)).unwrap_or(0);
            if let Some(input) = queues[at].1.pop_front() {
                out.push(input);
            }
            if queues[at].1.is_empty() {
                queues.remove(at);
            }
        }
        out
    }

    /// Moves the machine to `to`: halted, exported, imported into a fresh host there, and resumed. Nothing it held
    /// is lost, or the swap fails.
    pub fn swap(&mut self, to: &mut dyn Environment) -> Result<(), HostError> {
        self.machine.halt()?;
        let entries = self.machine.export()?;
        let mut next = to.import(entries)?;
        next.resume()?;
        self.machine = next;
        self.environment = to.name();
        self.environments.push(to.name());
        Ok(())
    }
}
