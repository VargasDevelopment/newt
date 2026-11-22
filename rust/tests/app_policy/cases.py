"""Authored full-state expectations and independent boundary input construction."""
from itertools import product

def state(**values):
    names = ['screen','resume','selected','page','presses','about','first','row','setting','zone','focus','notice']
    if set(values)-set(names):raise ValueError('unknown state field')
    return [values.get(name, 0) for name in names]
def wire(s, action=None):
    return ('W' if action is None else 'D') + ' ' + ' '.join(map(str, s + ([] if action is None else [action])))
def result(s, page, changed=None):
    return ('W ' if changed is None else f'D {changed} ') + ' '.join(map(str, s + [page]))

def independent():
    cases = [
        ('home-books', 'H 0', 'H 0 Books 6'), ('home-apps', 'H 1', 'H 1 Apps 5'),
        ('home-settings', 'H 2', 'H 2 Settings 2'),
        ('home-index-clamp', 'H 4294967295', 'H 2 Settings 2'),
        ('read', wire(state(), 5), result(state(screen=4), 0, 1)),
        ('previous-clamp', wire(state(), 0), result(state(), 0, 0)),
        ('next', wire(state(), 1), result(state(selected=1), 0, 1)),
        ('next-last', wire(state(selected=2), 1), result(state(selected=2), 0, 0)),
        ('sleep-sample-reading', wire(state(screen=1,selected=2,page=42,about=1,presses=4,first=1,row=1,zone=1,notice=2),4),
         result(state(screen=3,page=42),0,1)),
        ('sleep-epub-reading', wire(state(screen=4,selected=2,page=42,about=1,presses=4,first=1,row=1,zone=1,notice=2),4),
         result(state(screen=3,resume=4,selected=2,page=42),0,1)),
        ('wake-awake-preserves-all',wire(state(screen=1,resume=0,selected=2,page=42,presses=255,about=4,first=1,row=1,setting=8,zone=1,focus=253,notice=2)),
         result(state(screen=1,resume=0,selected=2,page=42,presses=255,about=4,first=1,row=1,setting=8,zone=1,focus=253,notice=2),3)),
        ('wake-epub',wire(state(screen=3,resume=4,selected=2,page=42)),
         result(state(screen=4,resume=4,selected=2,page=42),0)),
        ('wake-sample-home',wire(state(screen=3,resume=1,selected=2,page=42)),
         result(state(resume=1,selected=2,page=42),0)),
        ('sleep-already',wire(state(screen=3,resume=4,presses=255),4),
         result(state(screen=3,resume=4,presses=255),0,0)),
        ('open-list-back-owner',wire(state(screen=2,zone=1),3),result(state(screen=2,zone=1),0,0)),
        ('settings-unconfirmed-preserved',wire(state(screen=2,notice=2),1),result(state(screen=2,setting=1,notice=2),0,1)),
        ('settings-saved-note-cleared',wire(state(screen=2,notice=1),1),result(state(screen=2,setting=1),0,1)),
        ('settings-format-note-cleared',wire(state(screen=2,notice=3),1),result(state(screen=2,setting=1),0,1)),
        ('settings-about-next-quickstart',wire(state(screen=2,setting=8,notice=1),1),result(state(screen=2,setting=9),0,1)),
        ('settings-boundary-keeps-note',wire(state(screen=2,setting=9,notice=1),1),result(state(screen=2,setting=9,notice=1),0,0)),
        ('about-hidden-counter',wire(state(screen=2,about=1,presses=3),2),result(state(screen=2,about=1,presses=4),2,0)),
        ('about-fifth',wire(state(screen=2,about=1,presses=4),2),result(state(screen=2,about=1,presses=5,first=1),3,1)),
        ('about-byte-wrap',wire(state(screen=2,about=1,presses=255),2),result(state(screen=2,about=1),2,0)),
        ('about-other-action-resets-counter',wire(state(screen=2,about=1,presses=4),5),result(state(screen=2,about=1),2,0)),
        ('updated-back-home',wire(state(screen=2,about=4,first=1,row=1,setting=3,notice=2),3),result(state(),0,1)),
        ('first-back-keeps-about',wire(state(screen=2,about=3,first=1,presses=4),3),result(state(screen=2,about=3),6,1)),
        ('whats-new-back-row',wire(state(screen=2,about=3),3),result(state(screen=2,about=2),2,1)),
        ('stats-summary-does-not-cycle', 'G 0 2', 'G 0 0 0 0'),
        ('stats-days-cycle', 'G 1 2', 'G 1 5 1 1'),
        ('stats-invalid-view-no-cycle', 'G 255 2', 'G 0 255 0 63'),
        ('stats-invalid-view-no-op', 'G 255 3', 'G 0 255 0 63'),
        ('stats-window-last', 'G 249 2', 'G 1 253 1 63'),
        ('stats-window-wrap', 'G 253 2', 'G 1 1 1 0'),
        ('stats-view-resets-window', 'G 9 0', 'G 1 0 0 0'),
        ('stats-device-wrap',wire(state(screen=2,row=1,setting=4,focus=253,notice=2),2),result(state(screen=2,row=1,setting=4,focus=1,notice=2),4,1)),
        ('sleep-rows-select-owner',wire(state(screen=2,row=1,setting=2,focus=1,notice=7),2),result(state(screen=2,row=1,setting=2,focus=1,notice=7),7,0)),
        ('sleep-row-byte-domain',wire(state(screen=2,row=1,setting=2,focus=255),0),result(state(screen=2,row=1,setting=2,focus=254),7,1)),
        ('switch-select-owner',wire(state(screen=2,row=1,setting=5),2),result(state(screen=2,row=1,setting=5),5,0)),
        ('wifi-select-owner',wire(state(screen=2,setting=7),2),result(state(screen=2,setting=7),0,0)),
    ]
    return cases

def corpus():
    rows = [w for _,w,_ in independent()]
    for screen,resume,selected,page,action in product(range(7),range(7),(0,1,2,3,2**32-1),(0,1,2,2**32-1),range(6)):
        rows.append(wire(state(screen=screen,resume=resume,selected=selected,page=page),action))
    for about,first,row,zone,setting,notice,action in product(range(5),range(2),range(2),range(2),range(9),(0,1,2,3,4,7),range(6)):
        rows.append(wire(state(screen=2,about=about,first=first,row=row,zone=zone,setting=setting,notice=notice,presses=4,focus=2),action))
    for count,action in product(range(256),range(6)):
        rows.append(wire(state(screen=2,about=1,presses=count),action))
    for focus,action in product(range(256),range(6)):
        rows.append('G '+str(focus)+' '+str(action))
        for setting in (2,4): rows.append(wire(state(screen=2,row=1,setting=setting,focus=focus),action))
    for resume,about,first in product(range(7),range(5),range(2)):
        rows.append(wire(state(screen=3,resume=resume,selected=2,page=42,about=about,first=first)))
    return list(dict.fromkeys(rows))
