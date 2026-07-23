use crate::{AnalyticsPayload, PropertiesPayload};

// Analytics is disabled in this build (no-phone-home fork): every method
// below reports success without recording or sending anything anywhere.
pub struct Analytics<'a, R: tauri::Runtime, M: tauri::Manager<R>> {
    _manager: &'a M,
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<'a, R: tauri::Runtime, M: tauri::Manager<R>> Analytics<'a, R, M> {
    pub async fn event(&self, _payload: AnalyticsPayload) -> Result<(), crate::Error> {
        Ok(())
    }

    pub fn event_fire_and_forget(&self, _payload: AnalyticsPayload) {}

    pub fn set_disabled(&self, _disabled: bool) -> Result<(), crate::Error> {
        Ok(())
    }

    pub fn is_disabled(&self) -> Result<bool, crate::Error> {
        Ok(true)
    }

    pub async fn set_properties(&self, _payload: PropertiesPayload) -> Result<(), crate::Error> {
        Ok(())
    }

    pub async fn identify(
        &self,
        _user_id: impl Into<String>,
        _payload: PropertiesPayload,
    ) -> Result<(), crate::Error> {
        Ok(())
    }
}

pub trait AnalyticsPluginExt<R: tauri::Runtime> {
    fn analytics(&self) -> Analytics<'_, R, Self>
    where
        Self: tauri::Manager<R> + Sized;
}

impl<R: tauri::Runtime, T: tauri::Manager<R>> AnalyticsPluginExt<R> for T {
    fn analytics(&self) -> Analytics<'_, R, Self>
    where
        Self: Sized,
    {
        Analytics {
            _manager: self,
            _runtime: std::marker::PhantomData,
        }
    }
}
