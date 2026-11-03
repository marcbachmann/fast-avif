use crate::props::Rav1dDataProps;
use crate::shared_bytes::SharedBytes;

#[derive(Clone, Default)]
#[doc(hidden)]
pub struct Rav1dData {
    pub data: Option<SharedBytes>,
    pub m: Rav1dDataProps,
}

impl From<SharedBytes> for Rav1dData {
    fn from(data: SharedBytes) -> Self {
        let size = data.len();
        Self {
            data: Some(data),
            m: Rav1dDataProps {
                size,
                ..Default::default()
            },
        }
    }
}

impl AsRef<[u8]> for Rav1dData {
    fn as_ref(&self) -> &[u8] {
        match &self.data {
            Some(data) => data.as_ref(),
            None => &[],
        }
    }
}
