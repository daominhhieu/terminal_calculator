#!/usr/bin/env python3
"""
Windows Calculator – Programmer Mode

Layout (79 cols wide):
┌─────────────────────────────────────────────────────────────────────────────┐
│  DISPLAY  —  spans full width (79 cols)                                     │
├─────────────────────────────┬───────────────────────────────────────────────┤
│  BUTTONS  (39 cols)         │  BIT VIEW  (39 cols)                          │
│  same height as buttons     │  same height as buttons                       │
└─────────────────────────────┴───────────────────────────────────────────────┘

Run:  python3 calc_prog.py
Quit: q  or  Esc
"""

import curses

# ══════════════════════════════════════════════════════════════════════════════
#  STATE
# ══════════════════════════════════════════════════════════════════════════════
class CalcState:
    def __init__(self):
        self.value      = 0
        self.input_str  = "0"
        self.prev_value = 0
        self.operator   = None
        self.new_input  = True
        self.base       = 16
        self.word_size  = 64
        self.error      = False

    def mask(self):
        return (1 << self.word_size) - 1

    def signed_value(self):
        v = self.value & self.mask()
        if v >= (1 << (self.word_size - 1)):
            v -= (1 << self.word_size)
        return v

    def display_value(self):
        if self.error: return "Error"
        v = self.value & self.mask()
        if   self.base == 16: return format(v, 'X')
        elif self.base == 10: return str(self.signed_value())
        elif self.base == 8:  return format(v, 'o')
        else:                 return format(v, 'b') if v else '0'

    def set_base(self, b):
        self.base = b; self.new_input = True

    def set_word(self, w):
        self.word_size = w
        self.value &= (1 << w) - 1
        self.new_input = True

    def input_digit(self, ch):
        if self.error: return
        VALID = {16:'0123456789ABCDEF', 10:'0123456789',
                 8: '01234567',          2: '01'}
        ch = ch.upper()
        if ch not in VALID[self.base]: return
        if self.new_input:
            self.input_str = ch; self.new_input = False
        else:
            self.input_str = ('' if self.input_str == '0' else self.input_str) + ch
        try: self.value = int(self.input_str, self.base) & self.mask()
        except: pass

    def toggle_bit(self, bp):
        if 0 <= bp < self.word_size:
            self.value ^= (1 << bp); self.new_input = True

    def _apply(self):
        if not self.operator: return
        a, b = self.prev_value, self.value
        try:
            if   self.operator == '+':  r = a + b
            elif self.operator == '-':  r = a - b
            elif self.operator == '*':  r = a * b
            elif self.operator == '/':
                if b == 0: raise ZeroDivisionError
                r = int(a / b)
            elif self.operator == '%':
                if b == 0: raise ZeroDivisionError
                r = a % b
            elif self.operator == '&':  r = a & b
            elif self.operator == '|':  r = a | b
            elif self.operator == '^':  r = a ^ b
            elif self.operator == '<<': r = a << (b % self.word_size)
            elif self.operator == '>>': r = a >> (b % self.word_size)
            else: r = b
            self.value = r & self.mask()
        except ZeroDivisionError:
            self.error = True
        self.operator = None; self.new_input = True

    def press_op(self, op):
        if self.error: return
        if self.operator and not self.new_input: self._apply()
        self.prev_value = self.value
        self.operator   = op
        self.new_input  = True

    def press_eq(self):
        if self.error: return
        self._apply()

    def press_ce(self):
        self.value = 0; self.input_str = "0"
        self.new_input = True; self.error = False

    def press_c(self):
        self.press_ce(); self.operator = None; self.prev_value = 0

    def press_back(self):
        if self.error or self.new_input: return
        self.input_str = self.input_str[:-1] or '0'
        try: self.value = int(self.input_str, self.base) & self.mask()
        except: self.value = 0

    def press_negate(self):
        self.value = (-self.value) & self.mask(); self.new_input = True

    def press_not(self):
        self.value = (~self.value) & self.mask(); self.new_input = True

    def rol(self):
        v = self.value & self.mask()
        self.value = ((v << 1) | (v >> (self.word_size - 1))) & self.mask()
        self.new_input = True

    def ror(self):
        v = self.value & self.mask()
        self.value = ((v >> 1) | ((v & 1) << (self.word_size - 1))) & self.mask()
        self.new_input = True


# ══════════════════════════════════════════════════════════════════════════════
#  LAYOUT
# ══════════════════════════════════════════════════════════════════════════════

BTN_W  = 9        # chars per button cell
BTN_H  = 3        # rows per button row (top-border + label + bottom-border)
NCOLS  = 4        # button columns
GAP    = 1        # 1-char column between left and right panels

# Left panel  = 4 buttons × 9 + 3 gaps = 39
LEFT_W = NCOLS * BTN_W + (NCOLS - 1)   # 39

# Right panel = same width so total = 39 + 1 + 39 = 79 cols
RIGHT_W = LEFT_W                        # 39

# Full display spans both panels + gap
FULL_W  = LEFT_W + GAP + RIGHT_W       # 79

# Display section height (rows)
DISP_H = 8   # badges(1) + 4 base readouts(4) + blank(1) + main value(1) + border(1)

BUTTONS = [
    [("QWORD","WORD64",1),("DWORD","WORD32",1),("WORD","WORD16",1),("BYTE","WORD8", 1)],
    [("HEX",  "BASE16",1),("DEC", "BASE10",1),("OCT", "BASE8", 1),("BIN", "BASE2", 1)],
    [("RoL",  "ROL",   1),("RoR", "ROR",   1),("NOT", "NOT",   1),("AND", "AND",   1)],
    [("OR",   "OR",    1),("XOR", "XOR",   1),("<<", "LSH",   1),(">>", "RSH",   1)],
    [("A",    "A",     1),("B",   "B",     1),("C",   "C",     1),("D",   "D",     1)],
    [("E",    "E",     1),("F",   "F",     1),("Mod", "MOD",   1),("CE",  "CE",    1)],
    [("7",    "7",     1),("8",   "8",     1),("9",   "9",     1),("÷",   "DIV",   1)],
    [("4",    "4",     1),("5",   "5",     1),("6",   "6",     1),("×",   "MUL",   1)],
    [("1",    "1",     1),("2",   "2",     1),("3",   "3",     1),("−",   "SUB",   1)],
    [("±",    "NEG",   1),("0",   "0",     1),("C",   "CLR",   1),("+",   "ADD",   1)],
    [("⌫",    "BACK",  2),("=",   "EQ",    2)],
]

BTNS_H = len(BUTTONS) * BTN_H   # 22 rows — bit view must match this exactly

OP_MAP = {
    "AND":'&', "OR":'|', "XOR":'^', "LSH":'<<', "RSH":'>>',
    "ADD":'+', "SUB":'-', "MUL":'*', "DIV":'/',  "MOD":'%',
}

# ══════════════════════════════════════════════════════════════════════════════
#  COLOURS
# ══════════════════════════════════════════════════════════════════════════════
CP_BG    = 1   # display / panel background  (dark)
CP_MODE  = 2   # QWORD / HEX badge           (gray)
CP_SEL   = 3   # active badge                (yellow)
CP_OP    = 4   # arithmetic / logic ops      (blue)
CP_SPEC  = 5   # CE / CLR / NOT / shifts     (dark red)
CP_DIGIT = 6   # digit buttons               (mid-gray)
CP_BIT1  = 7   # bit = 1                     (yellow on dark)
CP_BIT0  = 8   # bit = 0                     (dark gray)
CP_DIM   = 9   # disabled digit              (very dark)
CP_EQ    = 10  # equals                      (teal)
CP_LBL   = 11  # cyan labels / hints
CP_DIVID = 12  # divider between panels      (slightly lighter dark)

def init_colors():
    curses.start_color()
    curses.use_default_colors()
    curses.init_pair(CP_BG,    curses.COLOR_WHITE,  234)
    curses.init_pair(CP_MODE,  curses.COLOR_WHITE,  240)
    curses.init_pair(CP_SEL,   curses.COLOR_BLACK,  220)
    curses.init_pair(CP_OP,    curses.COLOR_WHITE,   25)
    curses.init_pair(CP_SPEC,  curses.COLOR_WHITE,   88)
    curses.init_pair(CP_DIGIT, curses.COLOR_WHITE,  237)
    curses.init_pair(CP_BIT1,  curses.COLOR_BLACK,  220)
    curses.init_pair(CP_BIT0,  curses.COLOR_WHITE,  236)
    curses.init_pair(CP_DIM,   curses.COLOR_WHITE,  234)
    curses.init_pair(CP_EQ,    curses.COLOR_WHITE,   31)
    curses.init_pair(CP_LBL,   curses.COLOR_CYAN,   234)
    curses.init_pair(CP_DIVID, curses.COLOR_WHITE,  236)

def action_color(action):
    if action in ('WORD64','WORD32','WORD16','WORD8',
                  'BASE16','BASE10','BASE8', 'BASE2'): return CP_MODE
    if action in ('AND','OR','XOR','LSH','RSH',
                  'ADD','SUB','MUL','DIV','MOD'):       return CP_OP
    if action in ('CE','CLR','NEG','NOT','ROL','ROR','BACK'): return CP_SPEC
    if action == 'EQ':                                  return CP_EQ
    return CP_DIGIT


# ══════════════════════════════════════════════════════════════════════════════
#  TUI
# ══════════════════════════════════════════════════════════════════════════════
class CalcTUI:
    def __init__(self, stdscr):
        self.scr      = stdscr
        self.st       = CalcState()
        self.btn_hits = []   # (y1,x1,y2,x2, action)
        self.bit_hits = []   # (y, x1, x2, bit_pos)
        curses.curs_set(0)
        curses.mousemask(curses.ALL_MOUSE_EVENTS | curses.REPORT_MOUSE_POSITION)
        init_colors()
        self._loop()

    # ── event loop ─────────────────────────────────────────────────────────
    def _loop(self):
        while True:
            self._draw()
            key = self.scr.getch()
            if key == curses.KEY_MOUSE:
                try:
                    _, mx, my, _, bstate = curses.getmouse()
                    if bstate & (curses.BUTTON1_PRESSED | curses.BUTTON1_CLICKED):
                        self._click(mx, my)
                except: pass
            elif key in (27, ord('q')):
                break
            else:
                self._key(key)

    def _click(self, mx, my):
        for (y1, x1, y2, x2, action) in self.btn_hits:
            if y1 <= my <= y2 and x1 <= mx <= x2:
                self._do(action); return
        for (y, x1, x2, bp) in self.bit_hits:
            if my == y and x1 <= mx <= x2:
                self.st.toggle_bit(bp); return

    def _key(self, key):
        ch = chr(key).upper() if 0 < key < 256 else ''
        if ch in '0123456789ABCDEF':             self._do(ch)
        elif key in (10, curses.KEY_ENTER):      self._do('EQ')
        elif key in (curses.KEY_BACKSPACE, 127): self._do('BACK')
        elif ch == '+': self._do('ADD')
        elif ch == '-': self._do('SUB')
        elif ch == '*': self._do('MUL')
        elif ch == '/': self._do('DIV')
        elif ch == '%': self._do('MOD')
        elif ch == '&': self._do('AND')
        elif ch == '|': self._do('OR')
        elif key == ord('^'): self._do('XOR')
        elif ch == '=':       self._do('EQ')

    def _do(self, action):
        s = self.st
        if   action in '0123456789ABCDEF': s.input_digit(action)
        elif action == 'BASE16': s.set_base(16)
        elif action == 'BASE10': s.set_base(10)
        elif action == 'BASE8':  s.set_base(8)
        elif action == 'BASE2':  s.set_base(2)
        elif action == 'WORD64': s.set_word(64)
        elif action == 'WORD32': s.set_word(32)
        elif action == 'WORD16': s.set_word(16)
        elif action == 'WORD8':  s.set_word(8)
        elif action == 'CE':   s.press_ce()
        elif action == 'CLR':  s.press_c()
        elif action == 'BACK': s.press_back()
        elif action == 'NEG':  s.press_negate()
        elif action == 'NOT':  s.press_not()
        elif action == 'ROL':  s.rol()
        elif action == 'ROR':  s.ror()
        elif action == 'EQ':   s.press_eq()
        elif action in OP_MAP: s.press_op(OP_MAP[action])

    # ── low-level draw helpers ──────────────────────────────────────────────
    def _put(self, y, x, text, attr=0):
        sh, sw = self.scr.getmaxyx()
        if y < 0 or y >= sh or x < 0 or x >= sw: return
        text = text[:max(0, sw - x)]
        if not text: return
        try: self.scr.addstr(y, x, text, attr)
        except: pass

    def _fill(self, y, x, h, w, attr):
        row = ' ' * w
        for dy in range(h):
            self._put(y + dy, x, row, attr)

    def _hline(self, y, x, w, ch='─', attr=0):
        self._put(y, x, ch * w, attr)

    # ══════════════════════════════════════════════════════════════════════
    #  MAIN DRAW
    # ══════════════════════════════════════════════════════════════════════
    def _draw(self):
        self.scr.erase()
        self.btn_hits = []
        self.bit_hits = []

        OX = 0   # start at left edge — no horizontal centering
        OY = 0   # start at top — no vertical centering, allows scrollback

        # ── Row 1: wide display spanning full width ──────────────────────
        self._draw_display(OY, OX)

        # ── Row 2: buttons (left) + bit view (right), same vertical span ─
        body_y  = OY + DISP_H
        right_x = OX + LEFT_W + GAP

        self._draw_buttons(body_y, OX)
        self._draw_bits(body_y, right_x, BTNS_H)

        # 1-col divider between button area and bit view
        bg = curses.color_pair(CP_BG)
        self._fill(body_y, OX + LEFT_W, BTNS_H, GAP, bg)

        # ── Hint line ───────────────────────────────────────────────────
        hint_y = OY + DISP_H + BTNS_H
        hint   = ("q:quit")
        self._put(hint_y, OX, hint, curses.color_pair(CP_LBL))

        self.scr.refresh()

    # ── Wide display (FULL_W, DISP_H rows) ─────────────────────────────
    def _draw_display(self, oy, ox):
        s  = self.st
        W  = FULL_W
        bg = curses.color_pair(CP_BG)
        self._fill(oy, ox, DISP_H, W, bg)

        WLBL = {64:"QWORD", 32:"DWORD", 16:"WORD", 8:"BYTE"}
        BLBL = {16:"HEX", 10:"DEC", 8:"OCT", 2:"BIN"}

        # row 0 — mode badges (left) + operator indicator (right)
        wx = ox + 1
        self._put(oy, wx, f" {WLBL[s.word_size]} ",
                  curses.color_pair(CP_MODE) | curses.A_BOLD)
        wx += len(WLBL[s.word_size]) + 3
        self._put(oy, wx, f" {BLBL[s.base]} ",
                  curses.color_pair(CP_MODE) | curses.A_BOLD)
        if s.operator:
            self._put(oy, ox + W - 5, f"  {s.operator}  ",
                      curses.color_pair(CP_OP) | curses.A_BOLD)

        # rows 1-4 — all-base readouts, dim
        BASES = [(16,"HEX"),(10,"DEC"),(8,"OCT"),(2,"BIN")]
        row = 1
        for b, lbl in BASES:
            if b == s.base: continue
            v = s.value & s.mask()
            if   b == 16: rs = format(v,'X')
            elif b == 10: rs = str(s.signed_value())
            elif b == 8:  rs = format(v,'o')
            else:         rs = format(v,'b') if v else '0'
            max_w = W - len(lbl) - 5
            if len(rs) > max_w: rs = '…' + rs[-(max_w - 1):]
            self._put(oy + row, ox + 2, f"{lbl}  {rs}", bg | curses.A_DIM)
            row += 1

        # row 6 — main value, big, right-aligned, grouped every 4
        disp = s.display_value()
        if s.base in (16, 2) and not s.error:
            rev = list(reversed(disp)); grp = []
            for i, c in enumerate(rev):
                if i and i % 4 == 0: grp.append(' ')
                grp.append(c)
            disp = ''.join(reversed(grp))
        disp = disp[-(W - 2):]
        self._put(oy + 6, ox + W - len(disp) - 1, disp,
                  bg | curses.A_BOLD)

        # row 7 — bottom border
        self._hline(oy + DISP_H - 1, ox, W, '═', bg)

    # ── Button grid (left, LEFT_W wide) ────────────────────────────────
    def _draw_buttons(self, oy, ox):
        VALID = {16:set('0123456789ABCDEF'), 10:set('0123456789'),
                 8: set('01234567'),          2: set('01')}
        bg = curses.color_pair(CP_BG)

        for ri, row in enumerate(BUTTONS):
            col = 0
            for (label, action, span) in row:
                cell_w = span * BTN_W + (span - 1)   # total cols this button occupies
                bx     = ox + col * (BTN_W + 1)
                by     = oy + ri * BTN_H

                is_active    = (action == f"BASE{self.st.base}" or
                                action == f"WORD{self.st.word_size}")
                is_disabled  = (len(action) == 1 and
                                action in '0123456789ABCDEF' and
                                action not in VALID[self.st.base])

                if is_active:     attr = curses.color_pair(CP_SEL)   | curses.A_BOLD
                elif is_disabled: attr = curses.color_pair(CP_DIM)   | curses.A_DIM
                else:             attr = curses.color_pair(action_color(action))

                # ── bordered key: top/mid/bot ─────────────────────────────
                inner_w = cell_w - 2
                lbl     = label[:inner_w]
                pad     = inner_w - len(lbl)
                lpad    = pad // 2
                rpad    = pad - lpad

                self._put(by,     bx, '\u250c' + '\u2500' * inner_w + '\u2510', attr)
                self._put(by + 1, bx, '\u2502' + ' '*lpad + lbl + ' '*rpad + '\u2502',
                          attr | curses.A_BOLD)
                self._put(by + 2, bx, '\u2514' + '\u2500' * inner_w + '\u2518', attr)

                # 1-char background gap between adjacent button cells
                if col + span < NCOLS:
                    self._fill(by, bx + cell_w, BTN_H, 1, bg)

                self.btn_hits.append((by, bx, by + BTN_H - 1, bx + cell_w - 1, action))
                col += span

    # ── Bit view (right, RIGHT_W wide, exactly BTNS_H rows tall) ───────
    def _draw_bits(self, oy, ox, panel_h):
        """
        Bit grid layout — each 16-bit block uses 3 lines:
          line 0  position labels:  63 62 61 60 │ 59 58 57 56 │ …
          line 1  bit values:        0  0  0  0 │  0  0  0  0 │ …
          line 2  group labels:    [63:60]       │[59:56]      │ …

        A thin separator line sits between consecutive 16-bit blocks.
        Header occupies line 0.

        Cell = 2 chars.  4 groups × 4 bits × 2 + 3 '│' + 2 margin = 37 ≤ 39.
        """
        s    = self.st
        v    = s.value & s.mask()
        bits = s.word_size
        bg   = curses.color_pair(CP_BG)

        self._fill(oy, ox, panel_h, RIGHT_W, bg)

        # ── header ──────────────────────────────────────────────────────
        # hdr = "── BIT VIEW  (click to toggle) ──"
        # self._put(oy, ox + (RIGHT_W - len(hdr)) // 2, hdr,
        #           curses.color_pair(CP_DIVID) | curses.A_BOLD)

        # How many 16-bit rows fit and how many we need
        num_rows  = bits // 16          # 1(BYTE/WORD) .. 4(QWORD)
        # Each block: 3 content lines + 1 separator  = 4 lines; first has no leading sep
        # Total lines needed = 1 header + num_rows*3 + (num_rows-1) separators
        #                    = 1 + num_rows*3 + num_rows - 1
        #                    = num_rows*4
        # For QWORD: 4*4 = 16 lines + 1 header = 17 ≤ 22 (BTNS_H). 

        CELL   = 2      # chars per bit  ("0 " or "1 ")
        BPGRP  = 4      # bits per group
        NGRPS  = 4      # groups per 16-bit row
        left   = ox + 1

        for ri in range(num_rows):
            msb    = bits - ri * 16 - 1
            # lines within this block (relative to oy+1 for header offset)
            base_y = oy + 1 + ri * 4   # first line of this 16-bit block
            lbl_y  = base_y
            bit_y  = base_y + 1
            grp_y  = base_y + 2
            sep_y  = base_y + 3        # separator AFTER this block

            # separator BEFORE block (between blocks, not before first)
            if ri > 0:
                self._hline(lbl_y - 1, ox, RIGHT_W, '─', bg | curses.A_DIM)

            cx = left
            for gi in range(NGRPS):
                bit_hi   = msb - gi * BPGRP
                bit_lo   = bit_hi - BPGRP + 1
                grp_span = BPGRP * CELL   # = 8

                # position labels: hi at left, lo at right of the 8-char span
                hi_s    = str(bit_hi)
                lo_s    = str(bit_lo)
                lbl_row = hi_s.ljust(grp_span)
                lbl_row = lbl_row[:grp_span - len(lo_s)] + lo_s
                self._put(lbl_y, cx, lbl_row, curses.color_pair(CP_LBL))

                # bit cells
                for bi in range(BPGRP):
                    bp  = bit_hi - bi
                    bv  = (v >> bp) & 1 if 0 <= bp < bits else 0
                    bx  = cx + bi * CELL
                    self._put(bit_y, bx, f"{bv}",
                              curses.color_pair(CP_BIT1 if bv else CP_BIT0) | curses.A_BOLD)
                    self.bit_hits.append((bit_y, bx, bx + CELL - 1, bp))

                # group label below bits
                # g_lbl = f"[{bit_hi}:{bit_lo}]"
                # self._put(grp_y, cx, g_lbl.ljust(grp_span),
                #           curses.color_pair(CP_LBL) | curses.A_DIM)

                cx += grp_span
                # '│' separator between groups
                if gi < NGRPS - 1:
                    self._put(lbl_y, cx, ' ', bg)
                    self._put(bit_y, cx, '│', bg | curses.A_DIM)
                    self._put(grp_y, cx, ' ', bg)
                    cx += 1

        # footer hint at the bottom of the bit panel
        # footer_y = oy + panel_h - 1
        # self._put(footer_y, ox + 1, "MSB → LSB  grouped by 4",
        #           curses.color_pair(CP_LBL) | curses.A_DIM)


# ══════════════════════════════════════════════════════════════════════════════
def main(stdscr):
    CalcTUI(stdscr)

if __name__ == '__main__':
    curses.wrapper(main)
