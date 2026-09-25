use super::ClassState;

/// 버서커 런타임 상태
#[derive(Debug, Clone)]
pub struct BerserkerState {
    pub mp: f64,
    pub max_mp: f64,
    pub fury: f64,
    pub max_fury: f64,
    pub is_burst: bool,
}

impl BerserkerState {
    pub fn new(max_mp: f64, max_fury: f64) -> Self {
        Self {
            mp: max_mp,
            max_mp,
            fury: 0.0,
            max_fury,
            is_burst: false,
        }
    }
}

impl ClassState for BerserkerState {
    fn clone_box(&self) -> Box<dyn ClassState> {
        Box::new(self.clone())
    }

    fn get_resource(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.mp,
            "Fury" => self.fury,
            "IsBurst" => self.is_burst as u8 as f64,
            _ => 0.0,
        }
    }

    fn set_resource(&mut self, name: &str, value: f64) {
        match name {
            "Mp" => self.mp = value.clamp(0.0, self.max_mp),
            "Fury" => {
                self.fury = value.clamp(0.0, self.max_fury);
                self.is_burst = self.fury >= self.max_fury;
            }
            _ => {}
        }
    }

    fn get_resource_max(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.max_mp,
            "Fury" => self.max_fury,
            _ => 0.0,
        }
    }
}
