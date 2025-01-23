// MIT License
//
// Copyright (c) 2022 Eliza Weisman
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use crate::arch::Local;
use conquer_once::spin::OnceCell;
use core::{
    any::Any,
    cell::Cell,
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    task::{Context, Poll},
};
use maitake::{
    scheduler::{Injector, StaticScheduler, Stealer, TaskStub},
    task::JoinHandle,
};
use pin_project::pin_project;
use rand::{rngs::OsRng, Rng, RngCore, SeedableRng};

static SCHEDULER: Local<Cell<Option<&'static StaticScheduler>>> = Local::new(|| Cell::new(None));

static RUNTIME: Runtime = Runtime {
    cores: [const { OnceCell::uninit() }; MAX_CORES],
    initialized: AtomicUsize::new(0),
    injector: {
        static STUB_TASK: TaskStub = TaskStub::new();
        unsafe { Injector::new_with_static_stub(&STUB_TASK) }
    },
};

const MAX_CORES: usize = 32;

struct Runtime {
    cores: [OnceCell<StaticScheduler>; MAX_CORES],

    injector: Injector<&'static StaticScheduler>,
    initialized: AtomicUsize,
}

impl Runtime {
    fn active_cores(&self) -> usize {
        self.initialized.load(Ordering::Acquire)
    }

    fn new_scheduler(&self) -> (usize, &StaticScheduler) {
        let next = self.initialized.fetch_add(1, Ordering::AcqRel);
        assert!(next < MAX_CORES);
        let scheduler = self.cores[next]
            .try_get_or_init(StaticScheduler::new)
            .unwrap();
        (next, scheduler)
    }

    fn try_steal_from(
        &'static self,
        idx: usize,
    ) -> Option<Stealer<'static, &'static StaticScheduler>> {
        self.cores[idx].try_get().ok()?.try_steal().ok()
    }
}

pub struct Executor {
    id: usize,
    scheduler: &'static StaticScheduler,
    running: AtomicBool,
    rng: rand_xoshiro::Xoroshiro128PlusPlus,
}

impl Executor {
    pub fn new() -> Executor {
        let (id, scheduler) = RUNTIME.new_scheduler();
        Executor {
            id,
            scheduler,
            running: AtomicBool::new(false),
            rng: rand_xoshiro::Xoroshiro128PlusPlus::seed_from_u64(OsRng.next_u64()),
        }
    }

    #[inline]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    fn tick(&mut self) -> bool {
        let tick = self.scheduler.tick();

        super::timer::TIMER.advance_ticks(0);

        if tick.has_remaining {
            return true;
        }

        let stolen = self.try_steal();
        if stolen > 0 {
            return true;
        }

        false
    }

    pub fn run(&mut self) {
        struct CoreGuard;
        impl Drop for CoreGuard {
            fn drop(&mut self) {
                SCHEDULER.with(|scheduler| scheduler.set(None));
            }
        }

        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        SCHEDULER.with(|scheduler| scheduler.set(Some(self.scheduler)));
        let _unset = CoreGuard;

        loop {
            if self.tick() {
                continue;
            }

            if !self.is_running() {
                return;
            }

            crate::arch::enable_interrupts_and_halt();
        }
    }

    fn try_steal(&mut self) -> usize {
        const MAX_STEAL_ATTEMPTS: usize = 16;
        const MAX_STOLEN_PER_TICK: usize = 256;

        if let Ok(injector) = RUNTIME.injector.try_steal() {
            return injector.spawn_n(&self.scheduler, MAX_STOLEN_PER_TICK);
        }

        if cfg!(feature = "work-stealing") {
            for _ in 0..MAX_STEAL_ATTEMPTS {
                let active_cores = RUNTIME.active_cores();

                if active_cores <= 1 {
                    break;
                }

                let victim_idx = self.rng.gen_range(0..active_cores);

                if victim_idx == self.id {
                    continue;
                }

                if let Some(victim) = RUNTIME.try_steal_from(victim_idx) {
                    let num_steal =
                        core::cmp::min(victim.initial_task_count() / 2, MAX_STOLEN_PER_TICK);
                    return victim.spawn_n(&self.scheduler, num_steal);
                }
            }
        }

        if let Ok(injector) = RUNTIME.injector.try_steal() {
            return injector.spawn_n(&self.scheduler, MAX_STOLEN_PER_TICK);
        }

        0
    }
}

#[pin_project]
pub struct CatchUnwind<F> {
    #[pin]
    future: F,
}

impl<F> Future for CatchUnwind<F>
where
    F: Future,
{
    type Output = Result<F::Output, Box<dyn Any + Send>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let f = self.project().future;
        match unwinding::panic::catch_unwind(|| f.poll(cx)) {
            Ok(v) => v.map(Ok),
            Err(e) => {
                crate::panic::inspect(&*e);
                Poll::Ready(Err(e))
            }
        }
    }
}

pub fn spawn<F>(future: F) -> JoinHandle<<CatchUnwind<F> as Future>::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let future = CatchUnwind { future };
    SCHEDULER.with(|scheduler| {
        if let Some(scheduler) = scheduler.get() {
            scheduler.spawn(future)
        } else {
            RUNTIME.injector.spawn(future)
        }
    })
}
