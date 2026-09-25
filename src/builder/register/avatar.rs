use crate::builder::dto::BuildRequest;
use crate::builder::stat::StatBuilder;

pub fn register(stat: &mut StatBuilder, req: &BuildRequest) {
    let bonus = req.avatar_main_stat_percent / 100.0;
    if bonus == 0.0 {
        return;
    }
    stat.add_additive("avatar", move |s| s.main_stat_mul += bonus);
}
