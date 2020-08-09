use alloc::rc::Rc;
use core::cell::RefCell;

// Currently, onbly a dummy implementation
pub struct VirtioNetDriver;

impl VirtioNetDriver {
    pub fn init_vqs(&mut self) {
    }

    pub fn set_polling_mode(&mut self, value: bool) {
        //(self.vqueues.as_deref_mut().unwrap())[VIRTIO_NET_RX_QUEUE].set_polling_mode(value);
    }

    pub fn get_mac_address(&self) -> [u8; 6] {
        [0; 6]
    }

    pub fn get_mtu(&self) -> u16 {
        1500 //self.device_cfg.mtu
    }

    pub fn get_tx_buffer(&mut self, len: usize) -> Result<(*mut u8, usize), ()> {
        Err(())
    }

    pub fn send_tx_buffer(&mut self, index: usize, len: usize) -> Result<(), ()> {
        Err(())
    }

    pub fn has_packet(&self) -> bool {
        false
    }

    pub fn receive_rx_buffer(&self) -> Result<&'static [u8], ()> {
        Err(())
    }

    pub fn rx_buffer_consumed(&mut self) {
    }
}

pub fn get_network_driver() -> Option<Rc<RefCell<VirtioNetDriver>>> {
    None
}
