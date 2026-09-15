//! TLS slot model proofs.

#[cfg(test)]
mod tests {
    use crate::tls;

    #[test]
    fn in_recursive_setup_false_by_default_after_init() {
        crate::init();
        // After init, bootstrap flags are cleared; this thread is not CREATE_OWNER.
        let _ = tls::in_recursive_setup();
    }
}

#[cfg(kani)]
mod kani_proofs {
    use crate::os::{self, TlsSlot};
    use crate::tls;
    use core::ffi::c_void;
    use core::sync::atomic::Ordering;

    #[kani::proof]
    fn tls_set_get_same_thread() {
        unsafe {
            let s = TlsSlot::new();
            assert!(s.get().is_null());
            s.ensure(None);
            let p = 0x100 as *mut c_void;
            s.set(p);
            assert_eq!(s.get(), p);
        }
    }

    #[kani::proof]
    fn tls_isolated_across_switch() {
        unsafe {
            let s = TlsSlot::new();
            s.ensure(None);
            os::switch_thread(1);
            s.set(0x10 as *mut c_void);
            os::switch_thread(2);
            assert!(s.get().is_null());
            s.set(0x20 as *mut c_void);
            os::switch_thread(1);
            assert_eq!(s.get(), 0x10 as *mut c_void);
        }
    }

    #[kani::proof]
    fn tls_thread_exit_clears() {
        unsafe {
            let s = TlsSlot::new();
            s.ensure(None);
            s.set(0x30 as *mut c_void);
            os::thread_exit();
            assert!(s.get().is_null());
        }
    }

    #[kani::proof]
    fn bootstrap_owner_is_recursive() {
        os::switch_thread(1);
        tls::INIT_OWNER.store(1, Ordering::Release);
        tls::IN_BOOTSTRAP.store(false, Ordering::Release);
        tls::CREATE_OWNER.store(0, Ordering::Release);
        assert!(tls::in_recursive_setup());
        os::switch_thread(2);
        assert!(!tls::in_recursive_setup());
        os::switch_thread(1);
        tls::INIT_OWNER.store(0, Ordering::Release);
        tls::IN_BOOTSTRAP.store(true, Ordering::Release);
        tls::BOOTSTRAP_TID.store(1, Ordering::Release);
        assert!(tls::in_recursive_setup());
        os::switch_thread(2);
        assert!(!tls::in_recursive_setup());
    }
}
