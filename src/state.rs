/// Pure calculator state — no UI dependencies.
#[derive(Clone, Debug)]
pub struct CalcState {
    pub value:      u64,
    pub prev:       u64,
    pub input_str:  String,
    pub operator:   Option<Op>,
    pub new_input:  bool,
    pub base:       Base,
    pub word:       WordSize,
    pub error:      bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base { Hex = 16, Dec = 10, Oct = 8, Bin = 2 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordSize { QWord = 64, DWord = 32, Word = 16, Byte = 8 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op { Add, Sub, Mul, Div, Mod, And, Or, Xor, Lsh, Rsh }

impl Base {
    pub fn label(self) -> &'static str {
        match self { Base::Hex=>"HEX", Base::Dec=>"DEC", Base::Oct=>"OCT", Base::Bin=>"BIN" }
    }
    pub fn radix(self) -> u32 { self as u32 }
    pub fn valid_char(self, c: char) -> bool {
        match self {
            Base::Hex => c.is_ascii_hexdigit(),
            Base::Dec => c.is_ascii_digit(),
            Base::Oct => ('0'..='7').contains(&c),
            Base::Bin => c == '0' || c == '1',
        }
    }
}

impl WordSize {
    pub fn label(self) -> &'static str {
        match self { WordSize::QWord=>"QWORD", WordSize::DWord=>"DWORD",
                     WordSize::Word=>"WORD",   WordSize::Byte=>"BYTE" }
    }
    pub fn bits(self) -> u32 { self as u32 }
    pub fn mask(self) -> u64 {
        match self {
            WordSize::QWord => u64::MAX,
            _ => (1u64 << self.bits()) - 1,
        }
    }
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::Add=>"+" , Op::Sub=>"-" , Op::Mul=>"×" , Op::Div=>"÷" , Op::Mod=>"%",
            Op::And=>"&" , Op::Or =>"|" , Op::Xor=>"^" , Op::Lsh=>"<<", Op::Rsh=>">>",
        }
    }
}

impl Default for CalcState {
    fn default() -> Self {
        Self {
            value:     0,
            prev:      0,
            input_str: "0".into(),
            operator:  None,
            new_input: true,
            base:      Base::Hex,
            word:      WordSize::QWord,
            error:     false,
        }
    }
}

impl CalcState {
    pub fn mask(&self) -> u64 { self.word.mask() }

    pub fn signed_value(&self) -> i64 {
        let v = self.value & self.mask();
        let bits = self.word.bits();
        if bits < 64 && v >= (1u64 << (bits - 1)) {
            (v as i64) - (1i64 << bits)
        } else {
            v as i64
        }
    }

    pub fn display_value(&self) -> String {
        if self.error { return "Error".into(); }
        let v = self.value & self.mask();
        match self.base {
            Base::Hex => format!("{:X}", v),
            Base::Dec => format!("{}", self.signed_value()),
            Base::Oct => format!("{:o}", v),
            Base::Bin => if v == 0 { "0".into() } else { format!("{:b}", v) },
        }
    }

    /// Format display value with spaces every 4 chars (for HEX/BIN)
    pub fn display_grouped(&self) -> String {
        let raw = self.display_value();
        if self.error { return raw; }
        match self.base {
            Base::Hex | Base::Bin => {
                let chars: Vec<char> = raw.chars().rev().collect();
                let mut out: Vec<char> = Vec::new();
                for (i, c) in chars.iter().enumerate() {
                    if i > 0 && i % 4 == 0 { out.push(' '); }
                    out.push(*c);
                }
                out.iter().rev().collect()
            }
            _ => raw,
        }
    }

    /// Value in any base as string (for readout panel)
    pub fn value_in(&self, base: Base) -> String {
        let v = self.value & self.mask();
        match base {
            Base::Hex => format!("{:X}", v),
            Base::Dec => format!("{}", self.signed_value()),
            Base::Oct => format!("{:o}", v),
            Base::Bin => if v == 0 { "0".into() } else { format!("{:b}", v) },
        }
    }

    pub fn set_base(&mut self, b: Base) {
        self.base = b;
        self.new_input = true;
    }

    pub fn set_word(&mut self, w: WordSize) {
        self.word = w;
        self.value &= self.mask();
        self.new_input = true;
    }

    pub fn input_digit(&mut self, c: char) {
        if self.error { return; }
        let cu = c.to_ascii_uppercase();
        if !self.base.valid_char(cu) { return; }
        if self.new_input {
            self.input_str = cu.to_string();
            self.new_input = false;
        } else {
            if self.input_str == "0" { self.input_str.clear(); }
            self.input_str.push(cu);
        }
        if let Ok(v) = u64::from_str_radix(&self.input_str, self.base.radix()) {
            self.value = v & self.mask();
        }
    }

    pub fn toggle_bit(&mut self, pos: u32) {
        if pos < self.word.bits() {
            self.value ^= 1u64 << pos;
            self.new_input = true;
        }
    }

    fn apply(&mut self) {
        let op = match self.operator.take() { Some(o) => o, None => return };
        let a = self.prev;
        let b = self.value & self.mask();
        let result: Option<u64> = match op {
            Op::Add => Some(a.wrapping_add(b)),
            Op::Sub => Some(a.wrapping_sub(b)),
            Op::Mul => Some(a.wrapping_mul(b)),
            Op::Div => if b == 0 { None } else { Some(a.wrapping_div(b)) },
            Op::Mod => if b == 0 { None } else { Some(a % b) },
            Op::And => Some(a & b),
            Op::Or  => Some(a | b),
            Op::Xor => Some(a ^ b),
            Op::Lsh => Some(a.wrapping_shl(b as u32 % self.word.bits())),
            Op::Rsh => Some(a.wrapping_shr(b as u32 % self.word.bits())),
        };
        match result {
            Some(v) => { self.value = v & self.mask(); }
            None    => { self.error = true; }
        }
        self.new_input = true;
    }

    pub fn press_op(&mut self, op: Op) {
        if self.error { return; }
        if self.operator.is_some() && !self.new_input { self.apply(); }
        self.prev     = self.value & self.mask();
        self.operator = Some(op);
        self.new_input = true;
    }

    pub fn press_eq(&mut self) {
        if self.error { return; }
        self.apply();
    }

    pub fn press_ce(&mut self) {
        self.value = 0;
        self.input_str = "0".into();
        self.new_input = true;
        self.error = false;
    }

    pub fn press_c(&mut self) {
        self.press_ce();
        self.operator = None;
        self.prev = 0;
    }

    pub fn press_back(&mut self) {
        if self.error || self.new_input { return; }
        self.input_str.pop();
        if self.input_str.is_empty() { self.input_str = "0".into(); }
        if let Ok(v) = u64::from_str_radix(&self.input_str, self.base.radix()) {
            self.value = v & self.mask();
        } else {
            self.value = 0;
        }
    }

    pub fn press_negate(&mut self) {
        let v = (self.value as i64).wrapping_neg() as u64;
        self.value = v & self.mask();
        self.new_input = true;
    }

    pub fn press_not(&mut self) {
        self.value = (!self.value) & self.mask();
        self.new_input = true;
    }

    pub fn press_rol(&mut self) {
        let bits = self.word.bits();
        let v    = self.value & self.mask();
        let top  = (v >> (bits - 1)) & 1;
        self.value = ((v << 1) | top) & self.mask();
        self.new_input = true;
    }

    pub fn press_ror(&mut self) {
        let bits = self.word.bits();
        let v    = self.value & self.mask();
        let bot  = v & 1;
        self.value = ((v >> 1) | (bot << (bits - 1))) & self.mask();
        self.new_input = true;
    }
}
