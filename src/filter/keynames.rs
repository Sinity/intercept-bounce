use input_linux_sys::{EV_ABS, EV_KEY, EV_LED, EV_MSC, EV_REL, EV_SYN};

static KEY_NAMES: phf::Map<u16, &'static str> = phf::phf_map! {
    0u16 => "KEY_RESERVED",
    1u16 => "KEY_ESC",
    2u16 => "KEY_1",
    3u16 => "KEY_2",
    4u16 => "KEY_3",
    5u16 => "KEY_4",
    6u16 => "KEY_5",
    7u16 => "KEY_6",
    8u16 => "KEY_7",
    9u16 => "KEY_8",
    10u16 => "KEY_9",
    11u16 => "KEY_0",
    12u16 => "KEY_MINUS",
    13u16 => "KEY_EQUAL",
    14u16 => "KEY_BACKSPACE",
    15u16 => "KEY_TAB",
    16u16 => "KEY_Q",
    17u16 => "KEY_W",
    18u16 => "KEY_E",
    19u16 => "KEY_R",
    20u16 => "KEY_T",
    21u16 => "KEY_Y",
    22u16 => "KEY_U",
    23u16 => "KEY_I",
    24u16 => "KEY_O",
    25u16 => "KEY_P",
    26u16 => "KEY_LEFTBRACE",
    27u16 => "KEY_RIGHTBRACE",
    28u16 => "KEY_ENTER",
    29u16 => "KEY_LEFTCTRL",
    30u16 => "KEY_A",
    31u16 => "KEY_S",
    32u16 => "KEY_D",
    33u16 => "KEY_F",
    34u16 => "KEY_G",
    35u16 => "KEY_H",
    36u16 => "KEY_J",
    37u16 => "KEY_K",
    38u16 => "KEY_L",
    39u16 => "KEY_SEMICOLON",
    40u16 => "KEY_APOSTROPHE",
    41u16 => "KEY_GRAVE",
    42u16 => "KEY_LEFTSHIFT",
    43u16 => "KEY_BACKSLASH",
    44u16 => "KEY_Z",
    45u16 => "KEY_X",
    46u16 => "KEY_C",
    47u16 => "KEY_V",
    48u16 => "KEY_B",
    49u16 => "KEY_N",
    50u16 => "KEY_M",
    51u16 => "KEY_COMMA",
    52u16 => "KEY_DOT",
    53u16 => "KEY_SLASH",
    54u16 => "KEY_RIGHTSHIFT",
    55u16 => "KEY_KPASTERISK",
    56u16 => "KEY_LEFTALT",
    57u16 => "KEY_SPACE",
    58u16 => "KEY_CAPSLOCK",
    59u16 => "KEY_F1",
    60u16 => "KEY_F2",
    61u16 => "KEY_F3",
    62u16 => "KEY_F4",
    63u16 => "KEY_F5",
    64u16 => "KEY_F6",
    65u16 => "KEY_F7",
    66u16 => "KEY_F8",
    67u16 => "KEY_F9",
    68u16 => "KEY_F10",
    69u16 => "KEY_NUMLOCK",
    70u16 => "KEY_SCROLLLOCK",
    71u16 => "KEY_KP7",
    72u16 => "KEY_KP8",
    73u16 => "KEY_KP9",
    74u16 => "KEY_KPMINUS",
    75u16 => "KEY_KP4",
    76u16 => "KEY_KP5",
    77u16 => "KEY_KP6",
    78u16 => "KEY_KPPLUS",
    79u16 => "KEY_KP1",
    80u16 => "KEY_KP2",
    81u16 => "KEY_KP3",
    82u16 => "KEY_KP0",
    83u16 => "KEY_KPDOT",
    87u16 => "KEY_F11",
    88u16 => "KEY_F12",
    96u16 => "KEY_KPENTER",
    97u16 => "KEY_RIGHTCTRL",
    98u16 => "KEY_KPSLASH",
    99u16 => "KEY_SYSRQ",
    100u16 => "KEY_RIGHTALT",
    102u16 => "KEY_HOME",
    103u16 => "KEY_UP",
    104u16 => "KEY_PAGEUP",
    105u16 => "KEY_LEFT",
    106u16 => "KEY_RIGHT",
    107u16 => "KEY_END",
    108u16 => "KEY_DOWN",
    109u16 => "KEY_PAGEDOWN",
    110u16 => "KEY_INSERT",
    111u16 => "KEY_DELETE",
    119u16 => "KEY_PAUSE",
    125u16 => "KEY_LEFTMETA",
    126u16 => "KEY_RIGHTMETA",
    127u16 => "KEY_COMPOSE",
};

#[inline]
pub fn get_key_name(code: u16) -> &'static str {
    KEY_NAMES.get(&code).copied().unwrap_or("UNKNOWN")
}

/// Resolve a key identifier (numeric code or symbolic name) to a key code.
/// The lookup is case-insensitive for symbolic names.
#[inline]
pub fn resolve_key_code(identifier: &str) -> Option<u16> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(code) = trimmed.parse::<u16>() {
        return Some(code);
    }

    let normalized = trimmed.to_ascii_uppercase();
    KEY_NAMES.entries().find_map(|(code, name)| {
        if name == &normalized {
            Some(*code)
        } else {
            None
        }
    })
}

#[inline]
pub fn get_event_type_name(type_: u16) -> &'static str {
    match i32::from(type_) {
        EV_SYN => "EV_SYN",
        EV_KEY => "EV_KEY",
        EV_REL => "EV_REL",
        EV_ABS => "EV_ABS",
        EV_MSC => "EV_MSC",
        EV_LED => "EV_LED",
        _ => "Unknown",
    }
}

#[inline]
pub fn get_value_name(value: i32) -> &'static str {
    match value {
        0 => "Release",
        1 => "Press",
        2 => "Repeat",
        _ => "Unknown",
    }
}
