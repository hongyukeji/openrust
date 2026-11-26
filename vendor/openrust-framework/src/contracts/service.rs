/// 将服务对象复位到可再次使用的状态。
pub trait ResetInterface {
    fn reset(&mut self);
}
