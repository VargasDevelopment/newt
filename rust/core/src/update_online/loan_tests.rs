use super::*;

struct Clock;
impl super::Clock for Clock { fn millis(&mut self) -> u32 { 0 } }
struct Client { replies: [i32; 3], at: usize, needs_consumer: bool, closed: bool }
impl Http for Client {
    fn limit(&mut self, _: u32, _: &mut dyn super::Clock) {}
    fn open(&mut self, _: &CStr, _: bool, _: &mut HttpHead, _: &mut [u8; URL_BYTES], _: &mut dyn super::Clock) -> bool { false }
    fn read(&mut self, bytes: &mut [u8], _: &mut dyn super::Clock) -> i32 {
        if self.closed || self.needs_consumer { return -1; }
        let got = self.replies[self.at]; self.at += 1;
        if got > 0 {
            bytes[..got as usize].fill(self.at as u8);
            self.needs_consumer = true;
        }
        got
    }
    fn close(&mut self) { self.closed = true; }
}

#[test]
fn consumer_can_mutate_same_client_between_reads() {
    let mut client = Client { replies: [2, 3, 0], at: 0, needs_consumer: false, closed: false };
    let mut buffer = [0; READ_BYTES]; let mut delivered = 0;
    let end = read_body_with_http(&mut client, &mut Clock, &mut buffer, 5, &mut |http, bytes| {
        assert!(http.needs_consumer);
        assert!(bytes.iter().all(|byte| *byte == http.at as u8));
        delivered += bytes.len();
        http.needs_consumer = false;
        true
    });
    assert_eq!(end, BodyEnd::Whole);
    assert_eq!((delivered, client.at), (5, 3));
}

#[test]
fn refusing_consumer_closes_actual_client_without_another_read() {
    let mut client = Client { replies: [2, 3, 0], at: 0, needs_consumer: false, closed: false };
    let end = read_body_with_http(&mut client, &mut Clock, &mut [0; READ_BYTES], 5,
        &mut |http, _| { http.close(); false });
    assert_eq!(end, BodyEnd::Refused);
    assert!(client.closed);
    assert_eq!(client.at, 1);
}
