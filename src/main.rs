mod state;
mod layout;

use std::io;

use crossterm::{
    cursor,
    event::{
        self, DisableMouseCapture, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
        KeyEventKind,
    },
    execute, terminal,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};

use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table},
    Frame, Terminal,
};

use state::{Base, CalcState, Op, WordSize};
use layout::{BUTTONS, BTN_ROWS};

// ─── palette ────────────────────────────────────────────────────────────────
const C_BG:       Color = Color::Indexed(234);
const C_BADGE:    Color = Color::Indexed(240);
const C_SEL:      Color = Color::Indexed(220);
const C_OP_BG:    Color = Color::Indexed(25);
const C_SPEC_BG:  Color = Color::Indexed(88);
const C_DIGIT_BG: Color = Color::Indexed(237);
const C_EQ_BG:    Color = Color::Indexed(31);
const C_DIM_BG:   Color = Color::Indexed(234);
const C_BIT1_BG:  Color = Color::Indexed(220);
const C_BIT0_BG:  Color = Color::Indexed(236);
const C_EXPR_BG:  Color = Color::Indexed(235);
const C_CYAN:     Color = Color::Cyan;
const C_WHITE:    Color = Color::White;
const C_BLACK:    Color = Color::Black;
const C_DARK:     Color = Color::Indexed(242);
const C_RED:      Color = Color::Red;
const C_PANEL:    Color = Color::Indexed(236);  // toggle-button highlight

fn btn_styles(action: &str, active: bool, disabled: bool) -> (Style, Style) {
    // Returns (border_style, inner_style)
    // border_style: box-drawing chars use button color as FG, dark BG — no bg bleed
    // inner_style:  spaces + label use button color as full BG
    if disabled {
        let border = Style::default().fg(Color::Indexed(238)).bg(C_BG);
        let inner  = Style::default().fg(Color::Indexed(238)).bg(C_DIM_BG);
        return (border, inner);
    }
    if active {
        let border = Style::default().fg(C_SEL).bg(C_BG);
        let inner  = Style::default().fg(C_BLACK).bg(C_SEL).add_modifier(Modifier::BOLD);
        return (border, inner);
    }
    let bg = match action {
        "WORD64"|"WORD32"|"WORD16"|"WORD8"
        |"BASE16"|"BASE10"|"BASE8"|"BASE2" => C_BADGE,
        "AND"|"OR"|"XOR"|"LSH"|"RSH"
        |"ADD"|"SUB"|"MUL"|"DIV"|"MOD"    => C_OP_BG,
        "CE"|"CLR"|"NEG"|"NOT"|"ROL"|"ROR"|"BACK" => C_SPEC_BG,
        "EQ"                               => C_EQ_BG,
        _                                  => C_DIGIT_BG,
    };
    let border = Style::default().fg(bg).bg(C_BG);   // colored border on dark bg
    let inner  = Style::default().fg(C_WHITE).bg(bg).add_modifier(Modifier::BOLD);
    (border, inner)
}

// ─── hit regions ────────────────────────────────────────────────────────────
#[derive(Clone)]
struct BtnHit { x1:u16, y1:u16, x2:u16, y2:u16, action:&'static str }

#[derive(Clone)]
struct BitHit { y:u16, x1:u16, x2:u16, bit:u32 }

// Special clickable toggle buttons in the toolbar
#[derive(Clone, Copy, PartialEq)]
enum ToggleId { History, BitView, Bitwise }

#[derive(Clone)]
struct ToggleHit { x1:u16, y1:u16, x2:u16, y2:u16, id: ToggleId }

// ─── expression history ──────────────────────────────────────────────────────
#[derive(Clone)]
struct ExprEntry {
    lhs: String, op: String, rhs: String, result: String, complete: bool,
}

// ─── which button rows are "bitwise" (rows 2-3: RoL/RoR/NOT/AND and OR/XOR/Lsh/Rsh)
const BITWISE_ROWS: &[usize] = &[2, 3];

// ─── app ────────────────────────────────────────────────────────────────────
struct App {
    st:            CalcState,
    btn_hits:      Vec<BtnHit>,
    bit_hits:      Vec<BitHit>,
    toggle_hits:   Vec<ToggleHit>,
    history:       Vec<ExprEntry>,
    pending:       Option<ExprEntry>,
    // toggles (all hidden by default)
    show_history:  bool,
    show_bits:     bool,
    show_bitwise:  bool,
}

impl App {
    fn new() -> Self {
        Self {
            st:           CalcState::default(),
            btn_hits:     Vec::new(),
            bit_hits:     Vec::new(),
            toggle_hits:  Vec::new(),
            history:      Vec::new(),
            pending:      None,
            show_history: false,
            show_bits:    false,
            show_bitwise: false,
        }
    }

    fn fmt(st: &CalcState, v: u64) -> String {
        let m = v & st.mask();
        match st.base {
            Base::Hex => format!("{:X}", m),
            Base::Dec => {
                let bits = st.word.bits();
                let sv = if bits < 64 && m >= (1u64 << (bits-1)) {
                    (m as i64) - (1i64 << bits)
                } else { m as i64 };
                format!("{}", sv)
            }
            Base::Oct => format!("{:o}", m),
            Base::Bin => if m == 0 { "0".into() } else { format!("{:b}", m) },
        }
    }

    fn dispatch(&mut self, action: &str) {
        match action {
            // toggle panel keys (also sent from mouse clicks on toolbar)
            "TOGGLE_HISTORY" => { self.show_history = !self.show_history; return; }
            "TOGGLE_BITS"    => { self.show_bits    = !self.show_bits;    return; }
            "TOGGLE_BITWISE" => { self.show_bitwise = !self.show_bitwise; return; }

            a @ ("ADD"|"SUB"|"MUL"|"DIV"|"MOD"|"AND"|"OR"|"XOR"|"LSH"|"RSH") => {
                let op_sym = match a {
                    "ADD"=>"+","SUB"=>"-","MUL"=>"×","DIV"=>"÷","MOD"=>"%",
                    "AND"=>"&","OR" =>"|","XOR"=>"^","LSH"=>"<<","RSH"=>">>",
                    _ => "?",
                }.to_string();
                if self.st.operator.is_some() && !self.st.new_input {
                    if let Some(mut e) = self.pending.take() {
                        e.rhs = Self::fmt(&self.st, self.st.value);
                        self.push_hist(e);
                    }
                }
                let lhs = Self::fmt(&self.st, self.st.value);
                self.pending = Some(ExprEntry {
                    lhs, op: op_sym, rhs: String::new(),
                    result: String::new(), complete: false,
                });
                self.raw(action);
            }

            "EQ" => {
                if let Some(mut e) = self.pending.take() {
                    e.rhs = Self::fmt(&self.st, self.st.value);
                    self.raw("EQ");
                    e.result = Self::fmt(&self.st, self.st.value);
                    e.complete = true;
                    self.push_hist(e);
                } else {
                    self.raw("EQ");
                }
            }

            "CE"|"CLR" => { self.pending = None; self.raw(action); }
            _ => self.raw(action),
        }
    }

    fn push_hist(&mut self, e: ExprEntry) {
        self.history.push(e);
        if self.history.len() > 8 { self.history.remove(0); }
    }

    fn raw(&mut self, action: &str) {
        let s = &mut self.st;
        match action {
            d if d.len()==1 && d.chars().next().map(|c|c.is_ascii_hexdigit()).unwrap_or(false)
                => s.input_digit(d.chars().next().unwrap()),
            "BASE16"=>s.set_base(Base::Hex),"BASE10"=>s.set_base(Base::Dec),
            "BASE8" =>s.set_base(Base::Oct),"BASE2" =>s.set_base(Base::Bin),
            "WORD64"=>s.set_word(WordSize::QWord),"WORD32"=>s.set_word(WordSize::DWord),
            "WORD16"=>s.set_word(WordSize::Word), "WORD8" =>s.set_word(WordSize::Byte),
            "ADD"=>s.press_op(Op::Add),"SUB"=>s.press_op(Op::Sub),
            "MUL"=>s.press_op(Op::Mul),"DIV"=>s.press_op(Op::Div),
            "MOD"=>s.press_op(Op::Mod),"AND"=>s.press_op(Op::And),
            "OR" =>s.press_op(Op::Or), "XOR"=>s.press_op(Op::Xor),
            "LSH"=>s.press_op(Op::Lsh),"RSH"=>s.press_op(Op::Rsh),
            "EQ"  =>s.press_eq(),  "CE"  =>s.press_ce(), "CLR" =>s.press_c(),
            "BACK"=>s.press_back(),"NEG" =>s.press_negate(),"NOT"=>s.press_not(),
            "ROL" =>s.press_rol(), "ROR" =>s.press_ror(),
            _ => {}
        }
    }

    // ── keyboard ──────────────────────────────────────────────────────────
    fn handle_key(&mut self, ev: KeyEvent) -> bool {
        // ignore key-repeat and key-release — only act on Press
        if ev.kind != KeyEventKind::Press { return true; }
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        match ev.code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char('c') if ctrl        => return false,
            // panel toggles
            KeyCode::Char('h') | KeyCode::Char('H') => self.dispatch("TOGGLE_HISTORY"),
            KeyCode::Char('b') | KeyCode::Char('B') => self.dispatch("TOGGLE_BITS"),
            KeyCode::Char('w') | KeyCode::Char('W') => self.dispatch("TOGGLE_BITWISE"),
            KeyCode::Char(c) => {
                let cu = c.to_ascii_uppercase();
                match cu {
                    '0'..='9'|'A'..='F' => self.dispatch(&cu.to_string()),
                    '+'=>self.dispatch("ADD"), '-'=>self.dispatch("SUB"),
                    '*'=>self.dispatch("MUL"), '/'=>self.dispatch("DIV"),
                    '%'=>self.dispatch("MOD"), '&'=>self.dispatch("AND"),
                    '|'=>self.dispatch("OR"),  '^'=>self.dispatch("XOR"),
                    '='=>self.dispatch("EQ"),
                    _ => {}
                }
            }
            KeyCode::Enter     => self.dispatch("EQ"),
            KeyCode::Backspace => self.dispatch("BACK"),
            KeyCode::Delete    => self.dispatch("CE"),
            _ => {}
        }
        true
    }

    // ── mouse ─────────────────────────────────────────────────────────────
    fn handle_mouse(&mut self, ev: MouseEvent) {
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
            let (mx, my) = (ev.column, ev.row);
            // toggle toolbar
            let toggles = self.toggle_hits.clone();
            for h in toggles {
                if my >= h.y1 && my <= h.y2 && mx >= h.x1 && mx <= h.x2 {
                    match h.id {
                        ToggleId::History => self.dispatch("TOGGLE_HISTORY"),
                        ToggleId::BitView => self.dispatch("TOGGLE_BITS"),
                        ToggleId::Bitwise => self.dispatch("TOGGLE_BITWISE"),
                    }
                    return;
                }
            }
            // buttons
            let btns = self.btn_hits.clone();
            for h in btns {
                if my >= h.y1 && my <= h.y2 && mx >= h.x1 && mx <= h.x2 {
                    self.dispatch(h.action); return;
                }
            }
            // bit cells
            let bits = self.bit_hits.clone();
            for h in bits {
                if my == h.y && mx >= h.x1 && mx <= h.x2 {
                    self.st.toggle_bit(h.bit); return;
                }
            }
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    //  RENDER
    // ════════════════════════════════════════════════════════════════════════
    fn render(&mut self, f: &mut Frame) {
        self.btn_hits.clear();
        self.bit_hits.clear();
        self.toggle_hits.clear();

        let area = f.area();
        f.render_widget(Block::default().style(Style::default().bg(C_BG)), area);

        // ── rows: toolbar | [history] | display | body ────────────────────
        // toolbar=1, history=optional, display=fixed, body fills rest
        let mut constraints = vec![Constraint::Length(1)];
        if self.show_history { constraints.push(Constraint::Length(8)); }
        constraints.push(Constraint::Length(9));  // display panel
        constraints.push(Constraint::Min(0));     // body fills all remaining space

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let mut ri = 0;
        self.render_toolbar(f, rows[ri]); ri += 1;
        if self.show_history { self.render_history(f, rows[ri]); ri += 1; }
        self.render_display(f, rows[ri]); ri += 1;
        self.render_body(f, rows[ri]);
    }

    // ── toolbar: toggle buttons ───────────────────────────────────────────
    fn render_toolbar(&mut self, f: &mut Frame, area: Rect) {
        f.render_widget(
            Block::default().style(Style::default().bg(C_BG)),
            area,
        );

        // Build toggle chips
        struct Chip { label: String, on: bool, id: ToggleId }
        let chips = vec![
            Chip { label: "[H] History".into(), on: self.show_history, id: ToggleId::History },
            Chip { label: "[B] Bit View".into(), on: self.show_bits,   id: ToggleId::BitView },
            Chip { label: "[W] Bitwise Ops".into(), on: self.show_bitwise, id: ToggleId::Bitwise },
            // hint
        ];

        let mut x = area.x + 1;
        let y = area.y;

        for chip in &chips {
            let sty = if chip.on {
                Style::default().fg(C_BLACK).bg(C_SEL).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_DARK).bg(C_PANEL)
            };
            let txt = format!(" {} ", chip.label);
            let w   = txt.chars().count() as u16;
            if x + w >= area.x + area.width { break; }
            f.render_widget(
                Paragraph::new(txt.as_str()).style(sty),
                Rect { x, y, width: w, height: 1 },
            );
            self.toggle_hits.push(ToggleHit { x1:x, y1:y, x2:x+w-1, y2:y, id: chip.id });
            x += w + 1;
        }

        // right-aligned quit hint
        let hint = "q:quit ";
        let hx = area.x + area.width.saturating_sub(hint.len() as u16);
        f.render_widget(
            Paragraph::new(hint).style(Style::default().fg(C_DARK).bg(C_BG)),
            Rect { x: hx, y, width: hint.len() as u16, height: 1 },
        );
    }

    // ── history panel ─────────────────────────────────────────────────────
    fn render_history(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" History ")
            .title_style(Style::default().fg(C_CYAN).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(C_DARK).bg(C_EXPR_BG))
            .style(Style::default().bg(C_EXPR_BG));

        let inner = block.inner(area);
        f.render_widget(block, area);

        let max = inner.height as usize;
        let skip = self.history.len().saturating_sub(max);

        let lines: Vec<Line> = self.history.iter().skip(skip).map(|e| {
            let txt = if e.complete {
                format!("  {} {} {} = {}", e.lhs, e.op, e.rhs, e.result)
            } else {
                format!("  {} {}", e.lhs, e.op)
            };
            Line::from(Span::styled(txt, Style::default().fg(C_DARK).bg(C_EXPR_BG)))
        }).collect();

        // live pending expression at the bottom
        let mut all_lines = lines;
        if let Some(e) = &self.pending {
            let live_rhs = if !self.st.new_input {
                Self::fmt(&self.st, self.st.value)
            } else { String::new() };
            all_lines.push(Line::from(vec![
                Span::styled(format!("  {} ", e.lhs),
                    Style::default().fg(C_DARK).bg(C_EXPR_BG)),
                Span::styled(format!("{} ", e.op),
                    Style::default().fg(C_SEL).bg(C_EXPR_BG).add_modifier(Modifier::BOLD)),
                Span::styled(live_rhs,
                    Style::default().fg(C_WHITE).bg(C_EXPR_BG).add_modifier(Modifier::BOLD)),
            ]));
        }

        f.render_widget(
            Paragraph::new(all_lines)
                .style(Style::default().bg(C_EXPR_BG))
                .alignment(Alignment::Right),
            inner,
        );
    }

    // ── display (value + readouts) ────────────────────────────────────────
    fn render_display(&self, f: &mut Frame, area: Rect) {
        let s = &self.st;

        let outer = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(C_DARK).bg(C_BG))
            .style(Style::default().bg(C_BG));
        let inner = outer.inner(area);
        f.render_widget(outer, area);

        // split: readouts(left) | main value(right)
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(0)])
            .split(inner);

        // ── base readouts ──────────────────────────────────────────────────
        let rows: Vec<Row> = [Base::Hex, Base::Dec, Base::Oct, Base::Bin].iter().map(|&b| {
            let active = b == s.base;
            let lbl_sty = if active {
                Style::default().fg(C_BLACK).bg(C_SEL).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_DARK).bg(C_BG)
            };
            let val_sty = if active {
                Style::default().fg(C_WHITE).bg(C_BG).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_DARK).bg(C_BG)
            };
            Row::new(vec![
                Cell::from(format!(" {} ", b.label())).style(lbl_sty),
                Cell::from(format!(" {} ", s.value_in(b))).style(val_sty),
            ])
        }).collect();

        f.render_widget(
            Table::new(rows, [Constraint::Length(5), Constraint::Min(0)])
                .style(Style::default().bg(C_BG)),
            hsplit[0],
        );

        // ── main value ────────────────────────────────────────────────────
        let disp = s.display_grouped();
        let val_sty = if s.error {
            Style::default().fg(C_RED).bg(C_BG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(C_WHITE).bg(C_BG).add_modifier(Modifier::BOLD)
        };

        let badge_line = Line::from(vec![
            Span::styled(format!(" {} ", s.word.label()),
                Style::default().fg(C_BLACK).bg(C_SEL).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(C_BG)),
            Span::styled(format!(" {} ", s.base.label()),
                Style::default().fg(C_WHITE).bg(C_BADGE).add_modifier(Modifier::BOLD)),
        ]);

        // pending expression preview line (dim, above the value)
        let expr_line = if let Some(e) = &self.pending {
            let live = if !s.new_input { Self::fmt(s, s.value) } else { String::new() };
            Line::from(vec![
                Span::styled(format!("{} ", e.lhs), Style::default().fg(C_DARK).bg(C_BG)),
                Span::styled(format!("{} ", e.op),
                    Style::default().fg(C_SEL).bg(C_BG).add_modifier(Modifier::BOLD)),
                Span::styled(live, Style::default().fg(C_WHITE).bg(C_BG)),
            ])
        } else {
            Line::from("")
        };

        f.render_widget(
            Paragraph::new(vec![
                badge_line,
                expr_line,
                Line::default(),
                Line::from(Span::styled(disp, val_sty)),
            ])
            .style(Style::default().bg(C_BG))
            .alignment(Alignment::Right),
            hsplit[1],
        );
    }

    // ── body: buttons + optional bit view ────────────────────────────────
    fn render_body(&mut self, f: &mut Frame, area: Rect) {
        if self.show_bits {
            let hsplit = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(area);
            self.render_buttons(f, hsplit[0]);
            self.render_bits(f, hsplit[1]);
        } else {
            self.render_buttons(f, area);
        }
    }

    // ── button grid ───────────────────────────────────────────────────────
    fn render_buttons(&mut self, f: &mut Frame, area: Rect) {
        let valid_all = "0123456789ABCDEF";
        let valid: &str = match self.st.base {
            Base::Hex => "0123456789ABCDEF",
            Base::Dec => "0123456789",
            Base::Oct => "01234567",
            Base::Bin => "01",
        };

        // filter rows based on show_bitwise
        let visible_rows: Vec<(usize, &&[layout::BtnDef])> = BUTTONS.iter()
            .enumerate()
            .filter(|(ri, _)| self.show_bitwise || !BITWISE_ROWS.contains(ri))
            .collect();

        let n = visible_rows.len();
        let row_constraints: Vec<Constraint> =
            (0..n).map(|_| Constraint::Ratio(1, n as u32)).collect();
        let row_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(area);

        for ((_, row_def), &row_area) in visible_rows.iter().zip(row_areas.iter()) {
            let total_spans: u32 = row_def.iter().map(|b| b.2 as u32).sum();
            let col_constraints: Vec<Constraint> = row_def.iter()
                .map(|b| Constraint::Ratio(b.2 as u32, total_spans))
                .collect();
            let col_areas = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(col_constraints)
                .split(row_area);

            for (&(label, action, _), &btn_area) in row_def.iter().zip(col_areas.iter()) {
                let is_active = action == format!("BASE{}", self.st.base.radix())
                    || action == format!("WORD{}", self.st.word.bits());
                let is_disabled = label.len() == 1
                    && valid_all.contains(label)
                    && !valid.contains(label);

                let (bsty, lsty) = btn_styles(action, is_active, is_disabled);
                let Rect { x: bx, y: by, width: bw, height: bh } = btn_area;
                if bw < 2 || bh < 2 { continue; }

                // ── Render button by writing directly into the frame buffer ──
                // Avoids ratatui Paragraph filling entire rect with bg color.
                let iw = bw - 2;
                let ih = bh - 2;

                fn buf_set(buf: &mut ratatui::buffer::Buffer,
                           x: u16, y: u16, ch: char, sty: Style) {
                    let a = buf.area();
                    if x >= a.x && y >= a.y && x < a.x + a.width && y < a.y + a.height {
                        let s = ch.to_string();
                        buf[(x, y)].set_symbol(&s);
                        buf[(x, y)].set_style(sty);
                    }
                }

                {
                    let buf = f.buffer_mut();

                    // top row: ╭──╮
                    buf_set(buf, bx, by, '╭', bsty);
                    for dx in 1..bw-1 { buf_set(buf, bx+dx, by, '─', bsty); }
                    buf_set(buf, bx+bw-1, by, '╮', bsty);

                    // inner rows: │   │  filled with button bg
                    for row in 0..ih {
                        let ry = by + 1 + row;
                        buf_set(buf, bx, ry, '│', bsty);
                        for dx in 1..bw-1 { buf_set(buf, bx+dx, ry, ' ', lsty); }
                        buf_set(buf, bx+bw-1, ry, '│', bsty);
                    }

                    // bottom row: ╰──╯
                    buf_set(buf, bx, by+bh-1, '╰', bsty);
                    for dx in 1..bw-1 { buf_set(buf, bx+dx, by+bh-1, '─', bsty); }
                    buf_set(buf, bx+bw-1, by+bh-1, '╯', bsty);

                    // label centred on middle inner row
                    if ih > 0 {
                        let label_y   = by + 1 + ih / 2;
                        let lbl_chars: Vec<char> = label.chars().collect();
                        let lbl_w     = lbl_chars.len().min(iw as usize);
                        let lpad      = (iw as usize - lbl_w) / 2;
                        for dx in 1..bw-1 {
                            let ix = (dx - 1) as usize;
                            let ch = if ix >= lpad && ix < lpad + lbl_w {
                                lbl_chars[ix - lpad]
                            } else { ' ' };
                            buf_set(buf, bx+dx, label_y, ch, lsty);
                        }
                    }
                }
                self.btn_hits.push(BtnHit {
                    x1: btn_area.x, y1: btn_area.y,
                    x2: btn_area.x + btn_area.width.saturating_sub(1),
                    y2: btn_area.y + btn_area.height.saturating_sub(1),
                    action,
                });
            }
        }
    }

    // ── bit view ──────────────────────────────────────────────────────────
    fn render_bits(&mut self, f: &mut Frame, area: Rect) {
        let s    = &self.st;
        let v    = s.value & s.mask();
        let bits = s.word.bits();
        let num_rows = (bits / 16).max(1);

        let outer = Block::default()
            .title("Bit View")
            .title_style(Style::default().fg(C_CYAN).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(C_DARK).bg(C_BG))
            .style(Style::default().bg(C_BG));

        let inner = outer.inner(area);
        f.render_widget(outer, area);

        if inner.height == 0 { return; }

        // footer hint
        let footer_y = inner.y + inner.height - 1;
        // f.render_widget(
        //     Paragraph::new("MSB → LSB  grouped by 4")
        //         .style(Style::default().fg(C_DARK).bg(C_BG))
        //         .alignment(Alignment::Center),
        //     Rect { x: inner.x, y: footer_y, width: inner.width, height: 1 },
        // );

        let content_h = inner.height.saturating_sub(1);
        if content_h == 0 { return; }

        // Use Layout to distribute all content_h evenly across 16-bit rows
        // Each row has: [sep(1)] + lbl(1) + bits(1) + grp(1) = 3 or 4 lines
        // We interleave separator rows with content rows in the constraint list
        let mut bit_constraints: Vec<Constraint> = Vec::new();
        for ri in 0..num_rows {
            if ri > 0 { bit_constraints.push(Constraint::Length(1)); } // separator
            bit_constraints.push(Constraint::Length(1)); // pos labels
            bit_constraints.push(Constraint::Min(1));    // bit cells (stretches)
            bit_constraints.push(Constraint::Length(1)); // group labels
        }
        let content_area = Rect { x: inner.x, y: inner.y, width: inner.width, height: content_h };
        let row_slots = Layout::default()
            .direction(Direction::Vertical)
            .constraints(bit_constraints)
            .split(content_area);

        let g4: Vec<Constraint> = (0..4).map(|_| Constraint::Ratio(1,4)).collect();
        let mut slot = 0usize;

        for ri in 0..num_rows {
            let msb = bits - ri * 16 - 1;

            if ri > 0 {
                // separator
                f.render_widget(
                    Paragraph::new("─".repeat(inner.width as usize))
                        .style(Style::default().fg(C_DARK).bg(C_BG)),
                    row_slots[slot],
                );
                slot += 1;
            }

            let lbl_area = row_slots[slot];     slot += 1;
            let bit_area = row_slots[slot];     slot += 1;
            let grp_area = row_slots[slot];     slot += 1;

            let lbl_cols = Layout::default().direction(Direction::Horizontal).constraints(g4.clone()).split(lbl_area);
            let bit_cols = Layout::default().direction(Direction::Horizontal).constraints(g4.clone()).split(bit_area);
            let grp_cols = Layout::default().direction(Direction::Horizontal).constraints(g4.clone()).split(grp_area);

            for gi in 0..4usize {
                let bit_hi = msb - gi as u32 * 4;
                let bit_lo = bit_hi.saturating_sub(3);

                f.render_widget(
                    Paragraph::new(format!("{:<4}{:>4}", bit_hi, bit_lo))
                        .style(Style::default().fg(C_CYAN).bg(C_BG)),
                    lbl_cols[gi],
                );

                let cell_w = (bit_cols[gi].width / 4).max(1);
                for bi in 0..4u32 {
                    let bp = bit_hi - bi;
                    let bv = if bp < bits { (v >> bp) & 1 } else { 0 };
                    let cx = bit_cols[gi].x + bi as u16 * cell_w;
                    let cell = Rect { x: cx, y: bit_cols[gi].y, width: cell_w, height: 1 };
                    let (fg, bg) = if bv==1 { (C_BLACK,C_BIT1_BG) } else { (C_WHITE,C_BIT0_BG) };
                    f.render_widget(
                        Paragraph::new(format!("{:^width$}", bv, width=cell_w as usize))
                            .style(Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD))
                            .alignment(Alignment::Center),
                        cell,
                    );
                    self.bit_hits.push(BitHit {
                        y: cell.y, x1: cell.x,
                        x2: cell.x + cell.width.saturating_sub(1), bit: bp,
                    });
                }

                // f.render_widget(
                //     Paragraph::new(format!("[{}:{}]", bit_hi, bit_lo))
                //         .style(Style::default().fg(C_DARK).bg(C_BG)),
                //     grp_cols[gi],
                // );
            }
        }
    }
}

// ─── main ───────────────────────────────────────────────────────────────────
fn main() -> io::Result<()> {
    // raw mode + alternate screen — keeps UI at fixed coordinates
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;

    let backend  = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        term.draw(|f| app.render(f))?;

        match event::read()? {
            Event::Key(kev)   => { if !app.handle_key(kev) { break; } }
            Event::Mouse(mev) => app.handle_mouse(mev),
            Event::Resize(..) => { term.autoresize()?; }
            _ => {}
        }
    }

    // restore terminal
    terminal::disable_raw_mode()?;
    execute!(term.backend_mut(), DisableMouseCapture, cursor::Show, LeaveAlternateScreen)?;
    Ok(())
}