//! EXP-2's Rust stand-ins for PolkaVM (`T-008-03`): a step interpreter for the module-boundary suite's
//! `ModuleSpec`, one sandboxed and one outside program per provider category.
//!
//! Each program is compiled once. The binding writes a release's spec into [`SLOT`], a reserved block of the
//! program's writable data, before the release's digest is taken, so every release is its own PolkaVM blob while
//! its code is unchanged. The spec is a few lines of text (`binding/src/spec.rs` writes it):
//!
//! ```text
//! k <category>            p <provider>            v <version>
//! s <1 if it starts>      o <the outside operation>
//! out honest | out pad <bytes> | out unparseable | out kind | out tx | out field
//! step L s|o <operation> <secret>     step R s|o <state key>
//! step C <operation> <https://host:port>     step M <callee> <kind>
//! ```
//!
//! **The sandboxed part** runs the steps in order and renders the output. Its only channel out is `rt_outside`,
//! the contract's outside request. A lookup of its own (`step L s`) is sent as an outside request naming a
//! secret, which the contract's shape forbids, so the platform refuses it before it reaches the outside part.
//! Every other step is an outside request naming the step, carrying what the steps obtained so far.
//!
//! **The outside part** serves one outside request: it performs the step it names, through the binding's egress,
//! proxy, state and cross-module calls, and answers with what it obtained.
#![no_std]

/// The size of the spec slot, in bytes: a 16-byte marker, a 4-byte little-endian length, then the spec's text.
pub const SLOT_SIZE: usize = 4096;
const MAGIC: &[u8; 16] = b"RUTENT-SPEC-SLOT";
const TEXT: usize = 20;
const NONE: u64 = u64::MAX;

/// The spec slot. It is writable data so that neither the compiler nor the PolkaVM linker can fold its reads into
/// constants; it is filled with `.` rather than zeroes so that the linker keeps every byte in the blob.
pub static mut SLOT: [u8; SLOT_SIZE] = slot();

const fn slot() -> [u8; SLOT_SIZE] {
    let mut s = [b'.'; SLOT_SIZE];
    let mut i = 0;
    while i < MAGIC.len() {
        s[i] = MAGIC[i];
        i += 1;
    }
    s
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    trap()
}

/// Stops the program with a trap.
pub fn trap() -> ! {
    // SAFETY: `unimp` traps; nothing follows it.
    unsafe { core::arch::asm!("unimp", options(noreturn)) }
}

/// The sandboxed part's host functions: the operation in, the outside request, the result out.
pub mod sandboxed {
    #[polkavm_derive::polkavm_import]
    unsafe extern "C" {
        /// Copies the provider-role operation, in its envelope, into `ptr`; its length.
        pub fn rt_input(ptr: *mut u8, cap: u64) -> u64;
        /// Sends one outside request; the response's length, or `u64::MAX` if the platform refused it.
        pub fn rt_outside(req: *const u8, req_len: u64, resp: *mut u8, resp_cap: u64) -> u64;
        /// Returns the result body.
        pub fn rt_output(ptr: *const u8, len: u64);
    }
}

/// The outside part's host functions: the request in, the response out, and the binding's proxy, state, egress
/// and cross-module calls.
pub mod outside {
    #[polkavm_derive::polkavm_import]
    unsafe extern "C" {
        /// Copies the outside request being served into `ptr`; its length.
        pub fn rt_request(ptr: *mut u8, cap: u64) -> u64;
        /// Returns the outside response.
        pub fn rt_respond(ptr: *const u8, len: u64);
        /// Looks up a secret for an outside operation; the value's length, or `u64::MAX` if refused.
        pub fn rt_secret(
            op: *const u8,
            op_len: u64,
            name: *const u8,
            name_len: u64,
            out: *mut u8,
            cap: u64,
        ) -> u64;
        /// Reads a state entry; the value's length, or `u64::MAX` if there is none.
        pub fn rt_state(key: *const u8, key_len: u64, out: *mut u8, cap: u64) -> u64;
        /// Calls an endpoint under an outside operation, with a body; 0 if completed.
        pub fn rt_egress(
            op: *const u8,
            op_len: u64,
            url: *const u8,
            url_len: u64,
            body: *const u8,
            len: u64,
        ) -> u64;
        /// Calls another provider's module, asking it a kind; 0 if delivered.
        pub fn rt_call(callee: *const u8, callee_len: u64, kind: *const u8, kind_len: u64) -> u64;
    }
}

/// What hostile sandboxed parts reach for (scoring sheet `B-02`). Each is called only on a condition that never
/// holds, so the import is in the blob and admission sees it.
pub mod hostile {
    #[polkavm_derive::polkavm_import]
    unsafe extern "C" {
        fn clock_now() -> u64;
    }

    /// Imports the outside part's secret lookup into a sandboxed part.
    pub fn reach_for_a_secret() {
        let mut out = [0u8; 8];
        // SAFETY: the host function reads and writes only the buffers passed.
        unsafe {
            super::outside::rt_secret(b"x".as_ptr(), 1, b"x".as_ptr(), 1, out.as_mut_ptr(), 8)
        };
    }

    /// Imports a clock into a sandboxed part.
    pub fn read_a_clock() {
        // SAFETY: the host function takes no argument.
        unsafe { clock_now() };
    }
}

/// Defines a sandboxed part's exports, `start` and `handle`, for a category; optionally with a hostile reach.
#[macro_export]
macro_rules! sandboxed_part {
    ($category:expr) => {
        $crate::sandboxed_part!($category, $crate::nothing);
    };
    ($category:expr, $reach:path) => {
        polkavm_derive::min_stack_size!(131072);

        #[polkavm_derive::polkavm_export]
        extern "C" fn start() -> u32 {
            if $crate::never() {
                $reach();
            }
            $crate::start($category)
        }

        #[polkavm_derive::polkavm_export]
        extern "C" fn handle() {
            $crate::sandboxed_handle()
        }
    };
}

/// Defines an outside part's exports, `start` and `serve`, for a category.
#[macro_export]
macro_rules! outside_part {
    ($category:expr) => {
        polkavm_derive::min_stack_size!(131072);

        #[polkavm_derive::polkavm_export]
        extern "C" fn start() -> u32 {
            $crate::start($category)
        }

        #[polkavm_derive::polkavm_export]
        extern "C" fn serve() {
            $crate::outside_serve()
        }
    };
}

/// Does nothing: an honest part's reach.
pub fn nothing() {}

/// A condition the compiler can't decide and that never holds: the slot's first byte is its marker's.
pub fn never() -> bool {
    // SAFETY: a volatile read of one byte of the slot.
    unsafe { core::ptr::read_volatile(&raw const SLOT as *const u8) == 0 }
}

/// The release's spec, copied out of the slot.
pub struct Spec {
    bytes: [u8; SLOT_SIZE],
    len: usize,
}

/// One step of a spec.
#[derive(Clone, Copy)]
pub enum Step<'a> {
    /// Look up a secret: from the sandboxed part, or the outside part.
    Lookup {
        /// From the sandboxed part.
        sandboxed: bool,
        /// The outside operation.
        operation: &'a [u8],
        /// The secret's name.
        name: &'a [u8],
    },
    /// Read a state entry.
    Read {
        /// The entry.
        key: &'a [u8],
    },
    /// Call an endpoint.
    Call {
        /// The outside operation.
        operation: &'a [u8],
        /// `https://host:port`.
        url: &'a [u8],
    },
    /// Call another provider's module.
    Module {
        /// The callee.
        callee: &'a [u8],
        /// The kind asked.
        kind: &'a [u8],
    },
}

impl Spec {
    /// Copies the spec out of the slot.
    pub fn load() -> Self {
        let mut bytes = [0u8; SLOT_SIZE];
        let p = &raw const SLOT as *const u8;
        let mut i = 0;
        while i < SLOT_SIZE {
            // SAFETY: `i` is within the slot.
            bytes[i] = unsafe { core::ptr::read_volatile(p.add(i)) };
            i += 1;
        }
        let len = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
        let len = if bytes[..16] == *MAGIC && len <= SLOT_SIZE - TEXT {
            len
        } else {
            0
        };
        Spec { bytes, len }
    }

    fn lines(&self) -> impl Iterator<Item = &[u8]> {
        self.bytes[TEXT..TEXT + self.len]
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
    }

    /// The rest of the first line whose first word is `tag`.
    pub fn field(&self, tag: &[u8]) -> &[u8] {
        for line in self.lines() {
            let mut words = line.splitn(2, |b| *b == b' ');
            if words.next() == Some(tag) {
                return words.next().unwrap_or(b"");
            }
        }
        b""
    }

    /// The steps, in order.
    pub fn steps(&self) -> impl Iterator<Item = Step<'_>> {
        self.lines().filter_map(|line| {
            let mut w = line.split(|b| *b == b' ');
            if w.next()? != b"step" {
                return None;
            }
            match w.next()? {
                b"L" => {
                    let sandboxed = w.next()? == b"s";
                    Some(Step::Lookup {
                        sandboxed,
                        operation: w.next()?,
                        name: w.next()?,
                    })
                }
                b"R" => {
                    w.next()?;
                    Some(Step::Read { key: w.next()? })
                }
                b"C" => Some(Step::Call {
                    operation: w.next()?,
                    url: w.next()?,
                }),
                b"M" => Some(Step::Module {
                    callee: w.next()?,
                    kind: w.next()?,
                }),
                _ => None,
            }
        })
    }
}

/// Starts a part: it traps unless the spec is its category's and says it starts.
pub fn start(category: &[u8]) -> u32 {
    let spec = Spec::load();
    if spec.field(b"k") != category || spec.field(b"s") != b"1" {
        trap()
    }
    1
}

/// Writes into a fixed buffer; a write past its end traps.
pub struct W<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> W<'a> {
    /// An empty writer over `buf`.
    pub fn new(buf: &'a mut [u8]) -> Self {
        W { buf, len: 0 }
    }

    /// What was written.
    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Writes bytes as they are.
    pub fn raw(&mut self, b: &[u8]) {
        if self.len + b.len() > self.buf.len() {
            trap()
        }
        self.buf[self.len..self.len + b.len()].copy_from_slice(b);
        self.len += b.len();
    }

    /// Writes plain text escaped for a JSON string, without its quotes.
    pub fn esc(&mut self, b: &[u8]) {
        for c in b {
            match c {
                b'"' => self.raw(b"\\\""),
                b'\\' => self.raw(b"\\\\"),
                b'\n' => self.raw(b"\\n"),
                c if *c < 0x20 => self.raw(b"?"),
                c => self.raw(&[*c]),
            }
        }
    }

    /// Writes already-escaped JSON string content in quotes.
    pub fn quoted(&mut self, b: &[u8]) {
        self.raw(b"\"");
        self.raw(b);
        self.raw(b"\"");
    }

    /// Writes a decimal number.
    pub fn num(&mut self, mut n: usize) {
        let mut d = [0u8; 20];
        let mut i = d.len();
        loop {
            i -= 1;
            d[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        self.raw(&d[i..]);
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// The content of the first string `"key":"…"` in `hay`, still escaped.
pub fn find_str<'a>(hay: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut pat = [0u8; 64];
    let mut w = W::new(&mut pat);
    w.raw(b"\"");
    w.raw(key);
    w.raw(b"\":\"");
    let n = w.len;
    let at = find(hay, &pat[..n])? + n;
    string_at(hay, at)
}

fn string_at(hay: &[u8], at: usize) -> Option<&[u8]> {
    let mut i = at;
    while i < hay.len() {
        match hay[i] {
            b'\\' => i += 2,
            b'"' => return Some(&hay[at..i]),
            _ => i += 1,
        }
    }
    None
}

fn find_num(hay: &[u8], key: &[u8]) -> Option<usize> {
    let mut pat = [0u8; 64];
    let mut w = W::new(&mut pat);
    w.raw(b"\"");
    w.raw(key);
    w.raw(b"\":");
    let n = w.len;
    let mut i = find(hay, &pat[..n])? + n;
    let mut v = 0usize;
    let start = i;
    while i < hay.len() && hay[i].is_ascii_digit() {
        v = v * 10 + (hay[i] - b'0') as usize;
        i += 1;
    }
    (i > start).then_some(v)
}

/// Unescapes JSON string content into `w`, for the bodies the outside part sends.
fn unescape(w: &mut W<'_>, b: &[u8]) {
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            w.raw(if b[i + 1] == b'n' {
                b"\n"
            } else {
                &b[i + 1..i + 2]
            });
            i += 2;
        } else {
            w.raw(&b[i..i + 1]);
            i += 1;
        }
    }
}

const MAX_OBTAINED: usize = 16;

/// What the steps obtained, in order, as escaped JSON string content.
struct Obtained {
    buf: [u8; 4096],
    spans: [(usize, usize); MAX_OBTAINED],
    count: usize,
    used: usize,
}

impl Obtained {
    fn push(&mut self, v: &[u8]) {
        if self.count == MAX_OBTAINED || self.used + v.len() > self.buf.len() {
            trap()
        }
        self.buf[self.used..self.used + v.len()].copy_from_slice(v);
        self.spans[self.count] = (self.used, v.len());
        self.used += v.len();
        self.count += 1;
    }

    fn each(&self) -> impl Iterator<Item = &[u8]> {
        self.spans[..self.count]
            .iter()
            .map(|(at, len)| &self.buf[*at..*at + *len])
    }
}

/// The sandboxed part's `handle`: runs the steps, then renders and returns the output.
pub fn sandboxed_handle() {
    let spec = Spec::load();
    let mut input = [0u8; 8192];
    // SAFETY: the host writes at most `cap` bytes into the buffer.
    let n = unsafe { sandboxed::rt_input(input.as_mut_ptr(), input.len() as u64) } as usize;
    let input = &input[..n.min(8192)];
    let tx = find_str(input, b"transaction").unwrap_or(b"");
    let mut obtained = Obtained {
        buf: [0; 4096],
        spans: [(0, 0); MAX_OBTAINED],
        count: 0,
        used: 0,
    };
    for (i, step) in spec.steps().enumerate() {
        let mut req = [0u8; 8192];
        let mut w = W::new(&mut req);
        w.raw(b"{\"transaction\":");
        w.quoted(tx);
        w.raw(b",\"operation\":");
        match step {
            Step::Lookup {
                sandboxed: true,
                operation,
                name,
            } => {
                w.quoted(operation);
                w.raw(b",\"arguments\":{},\"secret\":");
                w.quoted(name);
                w.raw(b"}");
            }
            _ => {
                let operation = match step {
                    Step::Lookup { operation, .. } | Step::Call { operation, .. } => operation,
                    _ => spec.field(b"o"),
                };
                w.quoted(operation);
                w.raw(b",\"arguments\":{\"step\":");
                w.num(i);
                w.raw(b",\"carry\":[");
                for (n, v) in obtained.each().enumerate() {
                    if n > 0 {
                        w.raw(b",");
                    }
                    w.quoted(v);
                }
                w.raw(b"]}}");
            }
        }
        let mut resp = [0u8; 8192];
        let len = w.len as u64;
        // SAFETY: the host reads `len` bytes of the request and writes at most `cap` bytes of the response.
        let r = unsafe {
            sandboxed::rt_outside(req.as_ptr(), len, resp.as_mut_ptr(), resp.len() as u64)
        };
        if r != NONE {
            let resp = &resp[..(r as usize).min(8192)];
            if let Some(v) = find_str(resp, b"obtained") {
                obtained.push(v);
            }
        }
    }
    let mut out = [0u8; 16384];
    let len = render(&spec, input, &obtained, &mut out);
    // SAFETY: the host reads `len` bytes of the buffer.
    unsafe { sandboxed::rt_output(out.as_ptr(), len as u64) };
}

/// Renders the output the spec names, as compact JSON with its keys in order, into `out`; its length.
fn render(spec: &Spec, input: &[u8], obtained: &Obtained, out: &mut [u8]) -> usize {
    let output = spec.field(b"out");
    let mut w = W::new(out);
    match output {
        b"unparseable" => w.raw(b"not a result {"),
        b"kind" => honest(&mut w, spec, input, obtained, b"deposit", false, false, 0),
        b"tx" => honest(&mut w, spec, input, obtained, b"withdrawal", true, false, 0),
        b"field" => honest(&mut w, spec, input, obtained, b"withdrawal", false, true, 0),
        _ => {
            honest(
                &mut w,
                spec,
                input,
                obtained,
                b"withdrawal",
                false,
                false,
                0,
            );
            if let Some(size) = output.strip_prefix(b"pad ") {
                let size = size
                    .iter()
                    .fold(0usize, |v, d| v * 10 + (d.wrapping_sub(b'0')) as usize);
                let length = w.len;
                if size > length {
                    w.len = 0;
                    honest(
                        &mut w,
                        spec,
                        input,
                        obtained,
                        b"withdrawal",
                        false,
                        false,
                        size - length,
                    );
                }
            }
        }
    }
    w.len
}

#[allow(clippy::too_many_arguments)]
fn honest(
    w: &mut W<'_>,
    spec: &Spec,
    input: &[u8],
    obtained: &Obtained,
    kind: &[u8],
    another_transaction: bool,
    extra: bool,
    pad: usize,
) {
    let tx = find_str(input, b"transaction").unwrap_or(b"");
    let amount = find_str(input, b"amount").unwrap_or(b"");
    let asset = find_str(input, b"asset").unwrap_or(b"");
    let provider = spec.field(b"p");
    w.raw(b"{");
    if extra {
        w.raw(b"\"extra\":true,");
    }
    w.raw(b"\"kind\":");
    w.quoted(kind);
    w.raw(b",\"outcome\":{\"amount_in\":");
    w.quoted(amount);
    w.raw(b",\"amount_out\":");
    w.quoted(amount);
    w.raw(b",\"evidence\":[{\"by\":");
    w.quoted(provider);
    w.raw(b",\"grade\":3,\"reference\":\"");
    w.raw(provider);
    w.raw(b"-");
    w.raw(spec.field(b"v"));
    for v in obtained.each() {
        w.raw(b"-");
        w.raw(v);
    }
    for _ in 0..pad {
        w.raw(b"x");
    }
    w.raw(b"\"}],\"fee\":{\"amount\":\"0\",\"asset\":");
    w.quoted(asset);
    w.raw(b"},\"status\":\"succeeded\"},\"transaction\":\"");
    w.raw(tx);
    if another_transaction {
        w.raw(b"-another");
    }
    w.raw(b"\"}");
}

/// The outside part's `serve`: performs the step the request names, and answers with what it obtained.
pub fn outside_serve() {
    let spec = Spec::load();
    let mut req = [0u8; 8192];
    // SAFETY: the host writes at most `cap` bytes into the buffer.
    let n = unsafe { outside::rt_request(req.as_mut_ptr(), req.len() as u64) } as usize;
    let req = &req[..n.min(8192)];
    let tx = find_str(req, b"transaction").unwrap_or(b"");
    let op = find_str(req, b"operation").unwrap_or(b"");
    let mut value = [0u8; 2048];
    let mut got: Option<usize> = None;
    if let Some(step) = find_num(req, b"step").and_then(|i| spec.steps().nth(i)) {
        match step {
            Step::Lookup {
                operation, name, ..
            } => {
                // SAFETY: the host reads the two names and writes at most `cap` bytes of the value.
                let r = unsafe {
                    outside::rt_secret(
                        operation.as_ptr(),
                        operation.len() as u64,
                        name.as_ptr(),
                        name.len() as u64,
                        value.as_mut_ptr(),
                        value.len() as u64,
                    )
                };
                got = (r != NONE).then_some((r as usize).min(value.len()));
            }
            Step::Read { key } => {
                // SAFETY: the host reads the key and writes at most `cap` bytes of the value.
                let r = unsafe {
                    outside::rt_state(
                        key.as_ptr(),
                        key.len() as u64,
                        value.as_mut_ptr(),
                        value.len() as u64,
                    )
                };
                got = (r != NONE).then_some((r as usize).min(value.len()));
            }
            Step::Call { operation, url } => {
                let mut body = [0u8; 4096];
                let mut b = W::new(&mut body);
                if let Some(at) = find(req, b"\"carry\":[") {
                    let mut i = at + 9;
                    let mut first = true;
                    while i < req.len() && req[i] == b'"' {
                        let Some(s) = string_at(req, i + 1) else {
                            break;
                        };
                        if !first {
                            b.raw(b"\n");
                        }
                        unescape(&mut b, s);
                        first = false;
                        i += s.len() + 2;
                        if i < req.len() && req[i] == b',' {
                            i += 1;
                        }
                    }
                }
                let len = b.len as u64;
                // SAFETY: the host reads the three buffers.
                unsafe {
                    outside::rt_egress(
                        operation.as_ptr(),
                        operation.len() as u64,
                        url.as_ptr(),
                        url.len() as u64,
                        body.as_ptr(),
                        len,
                    )
                };
            }
            Step::Module { callee, kind } => {
                // SAFETY: the host reads the two names.
                unsafe {
                    outside::rt_call(
                        callee.as_ptr(),
                        callee.len() as u64,
                        kind.as_ptr(),
                        kind.len() as u64,
                    )
                };
            }
        }
    }
    let mut resp = [0u8; 8192];
    let mut w = W::new(&mut resp);
    w.raw(b"{\"transaction\":");
    w.quoted(tx);
    w.raw(b",\"operation\":");
    w.quoted(op);
    w.raw(b",\"outcome\":{\"status\":\"completed\",\"body\":{");
    if let Some(len) = got {
        w.raw(b"\"obtained\":\"");
        w.esc(&value[..len]);
        w.raw(b"\"");
    }
    w.raw(b"}}}");
    let len = w.len as u64;
    // SAFETY: the host reads `len` bytes of the buffer.
    unsafe { outside::rt_respond(resp.as_ptr(), len) };
}
