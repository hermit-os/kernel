macro_rules! virtio_bitflags {
    (
        $(#[$outer:meta])*
        $vis:vis struct $BitFlags:ident: $T:ty {
            $(
                $(#[$inner:ident $($args:tt)*])*
                const $Flag:tt = $value:expr;
            )*
        }

        $($t:tt)*
    ) => {
        #[cfg_attr(
            feature = "zerocopy",
            derive(
                zerocopy_derive::FromZeroes,
                zerocopy_derive::FromBytes,
                zerocopy_derive::AsBytes
            )
        )]
        #[derive(Default, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        $(#[$outer])*
        $vis struct $BitFlags($T);

        ::bitflags::bitflags! {
            impl $BitFlags: $T {
                $(
                    $(#[$inner $($args)*])*
                    const $Flag = $value;
                )*

                const _ = !0;
            }
        }

        impl ::core::fmt::Debug for $BitFlags {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                struct Inner<'a>(&'a $BitFlags);

                impl<'a> ::core::fmt::Debug for Inner<'a> {
                    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        if self.0.is_empty() {
                            f.write_str("0x0")
                        } else {
                            ::bitflags::parser::to_writer(self.0, f)
                        }
                    }
                }

                f.debug_tuple(::core::stringify!($BitFlags))
                    .field(&Inner(self))
                    .finish()
            }
        }

        virtio_bitflags! {
            $($t)*
        }
    };
    () => {};
}
