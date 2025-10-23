pub fn new_sid() -> String {
    // 21 chars nanoid; short, URL-safe
    nanoid::nanoid!()
}

