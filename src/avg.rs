pub struct Avg {
    accumulator: u64,
    count: u32,
    target: u32,
}

impl Avg {
    pub const fn new(target: u32) -> Self {
        Self {
            accumulator: 0,
            count: 0,
            target,
        }
    }

    pub fn update(&mut self, value: u16) -> Option<u32> {
        //accumulator will overflow after 4000 years
        self.accumulator += u64::from(value);
        //count overflow after 27h
        self.count += 1;

        if self.count == self.target {
            #[allow(clippy::cast_possible_truncation)]
            Some((self.accumulator / u64::from(self.count)) as u32)
        } else {
            None
        }
    }
}
