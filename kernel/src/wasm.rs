use wasmi::*;

struct WasiCioVecT {
    data: *mut u8,
    len: usize,
}

pub fn inner_run(wasm: &[u8]) -> Result<(), Error> {
    let engine = Engine::default();
    let mut store = Store::new(&engine, 42);
    let mut linker = Linker::new(&engine);

    let module = Module::new(&engine, wasm)?;

    linker.define(
        "wasi_snapshot_preview1",
        "fd_write",
        Func::wrap(
            &mut store,
            |mut caller: Caller<_>,
             _fd: i32,
             iovs: i32,
             iovs_len: i32,
             nwritten: i32|
             -> Result<i32, core::Trap> {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return Err(core::Trap::new("memory missing".to_string()));
                };
                let data = memory.data_mut(caller.as_context_mut());
                for i in 0..iovs_len {
                    let ptr = unsafe { data.as_ptr().add((iovs + (i * 8)) as usize) };
                    let iov = unsafe { &*(ptr as *const WasiCioVecT) };
                    let buf = unsafe { ::core::slice::from_raw_parts_mut(iov.data, iov.len) };
                    println!("iov={:?}", buf);
                    *unsafe { &mut *(data.as_ptr().add(nwritten as _) as *mut u32) } +=
                        buf.len() as u32;
                }
                Ok(0)
            },
        ),
    )?;

    linker.define(
        "wasi_snapshot_preview1",
        "environ_get",
        Func::wrap(&mut store, |_a: i32, _b: i32| -> i32 {
            println!("wasi::environ_get");
            0
        }),
    )?;

    linker.define(
        "wasi_snapshot_preview1",
        "environ_sizes_get",
        Func::wrap(&mut store, |_a: i32, _b: i32| {
            println!("wasi::environ_sizes_get");
            0
        }),
    )?;

    linker.define(
        "wasi_snapshot_preview1",
        "proc_exit",
        Func::wrap(&mut store, |a: i32| {
            println!("wasm exit {a}");
        }),
    )?;

    let instance = linker.instantiate(&mut store, &module)?.start(&mut store)?;

    let start = instance.get_func(&mut store, "_start").unwrap();
    start.call(&mut store, &[], &mut [])?;

    Ok(())
}

pub fn run(wasm: &'static [u8]) {
    crate::task::spawn(async {
        let r = inner_run(wasm);
        println!("wasm={:?}", r);
    });
}
