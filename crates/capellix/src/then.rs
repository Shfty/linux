/// Concrete value equivalent of [`Result::and_then`]
pub trait Then<R>: Sized {
    fn then(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}

impl<T: Sized, R> Then<R> for T {}
