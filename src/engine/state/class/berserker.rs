use super::ClassState;

/// 버서커 런타임 상태
#[derive(Debug, Clone)]
pub struct BerserkerState {
    pub mp: f64,
    pub max_mp: f64,
    pub fury: f64,
    pub max_fury: f64,
    pub is_burst: bool,
    pub ultimate_point:f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_ultimate_point_is_bounded_and_independent_of_fury_and_mana(){
        let mut s=BerserkerState::new(100.,200.);s.set_resource("Fury",80.);s.set_resource("Mp",40.);
        assert_eq!(s.get_resource_max("UltimatePoint"),10000.);assert_eq!(s.get_resource("UltimatePoint"),0.);
        s.set_resource("UltimatePoint",12000.);assert_eq!(s.get_resource("UltimatePoint"),10000.);
        s.set_resource("UltimatePoint",f64::NAN);assert_eq!(s.get_resource("UltimatePoint"),10000.);
        s.set_resource("UltimatePoint",-1.);assert_eq!(s.get_resource("UltimatePoint"),0.);
        assert_eq!(s.fury,80.);assert_eq!(s.mp,40.);
    }
}

impl BerserkerState {
    pub fn new(max_mp: f64, max_fury: f64) -> Self {
        Self {
            mp: max_mp,
            max_mp,
            fury: 0.0,
            max_fury,
            is_burst: false,
            ultimate_point:0.,
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
            "UltimatePoint" => self.ultimate_point,
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
            "UltimatePoint" if value.is_finite()=>self.ultimate_point=value.clamp(0.,10000.),
            _ => {}
        }
    }

    fn get_resource_max(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.max_mp,
            "Fury" => self.max_fury,
            "UltimatePoint" => 10000.,
            _ => 0.0,
        }
    }
}
