use rand::Rng;

pub struct Simulation {
    pub nodes: u64,
    pub credits: f64,
}

impl Simulation {
    pub fn new(nodes: u64) -> Self {
        Self {
            nodes,
            credits: 0.0,
        }
    }

    pub fn run(&mut self, steps: u64, out: &str) {
        let mut wtr = csv::Writer::from_path(out).unwrap();
        for i in 0..steps {
            let infl = self.step();
            wtr.write_record(&[i.to_string(), infl.to_string()])
                .unwrap();
        }
        wtr.flush().unwrap();
    }

    fn step(&mut self) -> f64 {
        let mut rng = rand::thread_rng();
        let inc: f64 = rng.gen_range(0.0..1.0);
        self.credits += inc;
        inc
    }
}
