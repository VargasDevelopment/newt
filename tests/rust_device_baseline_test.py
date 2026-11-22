"""Independent receipts ensure no-op or malformed timings cannot become wins."""
import copy,importlib.util,math,unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('device_baseline',Path(__file__).resolve().parents[1]/'tools/rust-device-baseline.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
def receipt(terminal='RENDER_RETURN gen=2'):
    common={'boot':1,'session':2,'request_id':3}
    return {'connection':{'closed':True},'protocol':{'records':[
        dict(common,direction='request',body='PRESS next',monotonic=10.0),
        dict(common,direction='response',body='ACCEPTED',monotonic=10.1),
        dict(common,direction='response',body='CONSUMED',monotonic=10.2),
        dict(common,direction='response',body=terminal,monotonic=11.3)]}}
class Evidence(unittest.TestCase):
    def test_real_completed_interval(self):
        got=m.extract(receipt(),['page_next'])[0]
        self.assertAlmostEqual(got['request_to_completion_ms'],1300)
        self.assertAlmostEqual(got['consumed_to_completion_ms'],1100)
        self.assertTrue(got['visual_completed'])
    def test_no_change_never_becomes_a_completed_turn(self):
        got=m.extract(receipt('NO_VISUAL_CHANGE gen=1'),['page_next'])[0]
        self.assertFalse(got['visual_completed'])
    def test_missing_or_cross_request_acknowledgement_rejected(self):
        missing=receipt();missing['protocol']['records'].pop(2)
        wrong=receipt();wrong['protocol']['records'][-1]['request_id']=4
        for report in (missing,wrong):
            with self.assertRaises(ValueError):m.extract(report,['page_next'])
    def test_invalid_timestamp_or_terminal_or_connection_rejected(self):
        for value in (9.9,float('nan'),float('inf'),-1):
            report=receipt();report['protocol']['records'][-1]['monotonic']=value
            with self.assertRaises(ValueError):m.extract(report,['page_next'])
        for terminal in ('RENDER_RETURN gen=bad','NO_VISUAL_CHANGE gen=1 junk'):
            with self.assertRaises(ValueError):m.extract(receipt(terminal),['page_next'])
        report=receipt();report['connection']['closed']=False
        with self.assertRaises(ValueError):m.extract(report,['page_next'])
if __name__=='__main__':unittest.main()
