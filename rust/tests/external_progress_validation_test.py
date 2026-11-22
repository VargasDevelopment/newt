"""Malformed responses must fail setup, not masquerade as semantic witnesses."""
import unittest
from external_progress_validation import validate
BASE=['0','2','123','nb0123456789abc','00'*73,'00','begin:nb0123456789abc:1,read:digest,end','nb0123456789abc/slot0=ff00','']
class Grammar(unittest.TestCase):
    def test_accepts_corrupt_records_and_incomplete_effects(self):
        for trace in ('','begin:other:0','put:slot0:00','end'):
            row=BASE.copy();row[6]=trace;validate('|'.join(row),'xp.load')
    def test_rejects_malformed_fields(self):
        for index,value in [(0,'garbage'),(0,'01'),(1,'13'),(2,str(1<<64)),(3,'bad/name'),(4,'00'),(4,'gg'*73),(5,'10x'),(6,'garbage'),(7,'x/a=ff,x/a=ff'),(7,'x/b=ff,x/a=00'),(8,'x/a=f')]:
            with self.subTest(index=index,value=value):
                row=BASE.copy();row[index]=value
                with self.assertRaises(ValueError):validate('|'.join(row),'xp.load')
    def test_save_result_domain_and_peer_extent(self):
        row=BASE.copy();row[0]='3'
        with self.assertRaises(ValueError):validate('|'.join(row),'xp.save')
        row=BASE.copy();row[4]=row[4][:112]+'04'+row[4][114:]
        with self.assertRaises(ValueError):validate('|'.join(row),'xp.peers')
if __name__=='__main__':unittest.main()
