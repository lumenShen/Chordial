use crate::PlatformKeyboardLayout;

pub(crate) struct AndroidKeyboardLayout;

impl PlatformKeyboardLayout for AndroidKeyboardLayout {
    fn id(&self) -> &str {
        "android"
    }

    fn name(&self) -> &str {
        "Android"
    }
}

pub(crate) fn modifiers_from_android(meta_state: i32) -> crate::Modifiers {
    const SHIFT: i32 = 1;
    const ALT: i32 = 2;
    const CTRL: i32 = 4096;
    const META: i32 = 65536;

    crate::Modifiers {
        shift: meta_state & SHIFT != 0,
        alt: meta_state & ALT != 0,
        control: meta_state & CTRL != 0,
        platform: meta_state & META != 0,
        function: false,
    }
}

pub(crate) fn keystroke_for_keycode(
    key_code: i32,
    modifiers: crate::Modifiers,
) -> crate::Keystroke {
    let key = match key_code {
        4 => "escape",
        19 => "up",
        20 => "down",
        21 => "left",
        22 => "right",
        61 => "tab",
        62 => "space",
        66 => "enter",
        67 => "backspace",
        69 => "-",
        70 => "=",
        71 => "[",
        72 => "]",
        73 => "\\",
        74 => ";",
        75 => "'",
        76 => "/",
        77 => ",",
        78 => ".",
        79 => "`",
        81 => "+",
        92 => "pageup",
        93 => "pagedown",
        111 => "escape",
        112 => "delete",
        113 => "left",
        114 => "right",
        115 => "capslock",
        122 => "home",
        123 => "end",
        code @ (7..=16) => match code - 7 {
            0 => "0",
            1 => "1",
            2 => "2",
            3 => "3",
            4 => "4",
            5 => "5",
            6 => "6",
            7 => "7",
            8 => "8",
            _ => "9",
        },
        code @ (29..=54) => match code - 29 {
            0 => "a",
            1 => "b",
            2 => "c",
            3 => "d",
            4 => "e",
            5 => "f",
            6 => "g",
            7 => "h",
            8 => "i",
            9 => "j",
            10 => "k",
            11 => "l",
            12 => "m",
            13 => "n",
            14 => "o",
            15 => "p",
            16 => "q",
            17 => "r",
            18 => "s",
            19 => "t",
            20 => "u",
            21 => "v",
            22 => "w",
            23 => "x",
            24 => "y",
            _ => "z",
        },
        _ => {
            return crate::Keystroke {
                modifiers,
                key: String::new(),
                key_char: None,
            };
        }
    };

    crate::Keystroke {
        modifiers,
        key: key.to_owned(),
        key_char: if key.len() == 1 {
            Some(key.to_owned())
        } else {
            None
        },
    }
}
