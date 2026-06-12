//! Throwaway: report which icon codepoints Segoe UI Symbol covers.

fn main() {
    let path = r"C:\Windows\Fonts\seguisym.ttf";
    let data = std::fs::read(path).expect("read font");
    let face = ttf_parser::Face::parse(&data, 0).expect("parse font");
    let candidates: &[(char, &str)] = &[
        ('\u{1F4C2}', "open folder"),
        ('\u{1F4BE}', "floppy save"),
        ('\u{1F527}', "wrench"),
        ('\u{1F5D1}', "wastebasket"),
        ('\u{1F50E}', "magnifier"),
        ('\u{26A0}', "warning"),
        ('\u{2714}', "check"),
        ('\u{2716}', "heavy x"),
        ('\u{2717}', "ballot x"),
        ('\u{25CF}', "black circle"),
        ('\u{2B24}', "big circle"),
        ('\u{27F3}', "clockwise gapped arrow"),
        ('\u{1F504}', "rotate arrows"),
        ('\u{21BB}', "clockwise open arrow"),
        ('\u{21C4}', "swap arrows"),
        ('\u{2194}', "left-right arrow"),
        ('\u{2BCC}', "obscure unequip arrow"),
        ('\u{23CF}', "eject"),
        ('\u{21A9}', "hook arrow"),
        ('\u{2B07}', "down arrow"),
        ('\u{2192}', "right arrow"),
        ('\u{00D7}', "times"),
        ('\u{2022}', "bullet"),
        ('\u{2699}', "gear"),
        ('\u{1F4C1}', "closed folder"),
    ];
    for (c, name) in candidates {
        let ok = face.glyph_index(*c).is_some();
        println!("{} U+{:04X} {:<26} {}", if ok { "OK " } else { "no " }, *c as u32, name, c);
    }
}
