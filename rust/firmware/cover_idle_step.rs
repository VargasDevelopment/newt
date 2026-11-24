// SPDX-License-Identifier: MIT
//! The existing 16B idle owner's exact visit/interruption budget.
use super::{job::{self,Targets,JobOwner},io::Control,session::verdict,
    CoverResult as R,CoverIdleState,CoverResume,CoverResumeOwner};
use newt_native_io::UiIo;
use super::heap::Heap;
use core::ffi::CStr;
pub fn prepare(state:&mut CoverIdleState,screen:u8,targets:&Targets<'_>,control:&dyn Control,now:fn()->u32)->Result<u8,R>{
    if screen!=state.screen{state.resume=None;*state=CoverIdleState::new();state.screen=screen;state.note_input(now());}
    if targets.view!=state.view{state.restart();state.view=targets.view;state.clear_settled_slots();state.note_input(now());}
    let all=((1u16<<targets.count.min(4))-1)as u8;
    if state.failures>4||targets.count==0||targets.count>4||state.settled_slots()&all==all{state.resume=None;return Err(R::Idle);}
    if control.stopped(){state.note_input(now());return Err(R::Cancelled);}
    if now().wrapping_sub(state.quiet_from)<if state.failures==4{15000}else{2000}{return Err(R::Idle);}
    let waiting=state.waiting(now());
    if waiting!=0&&(state.settled_slots()|waiting)>>targets.selected&1!=0{return Err(R::Idle);}
    Ok(waiting)
}
#[inline(never)]
fn interrupted(state:&mut CoverIdleState,resume:CoverResume,slot:u8,now:u32){
    state.note_input(now);
    if slot>=4||resume.book==0{state.failures+=1;return;}
    let bit=1u8<<slot;
    if state.book()!=bit||state.resume.as_ref().is_some_and(|r|r.book!=resume.book){state.restart();state.set_interruption(bit,0);}
    let count=state.interruptions()+1;
    if count>=7{state.mark_settled_slots(bit);state.mark_paused_retryable();state.restart();return;}
    state.set_interruption(bit,count);
    if state.resume.is_none(){state.resume=CoverResumeOwner::try_new();
        if state.resume.is_none(){state.mark_settled_slots(bit);state.mark_paused_retryable();state.restart();return;}}
    if let Some(record)=state.resume.as_mut(){**record=resume;}
}
pub fn step(state:&mut CoverIdleState,screen:u8,targets:&Targets<'_>,io:&UiIo,
    control:&dyn Control,heap:&Heap<'_>,now:fn()->u32,books:&CStr,covers:&CStr,
    published:&mut dyn FnMut())->R{
    let waiting=match prepare(state,screen,targets,control,now){Ok(waiting)=>waiting,Err(result)=>return result};
    let all=((1u16<<targets.count.min(4))-1)as u8;let peak=job::planned_peak();
    let admitted=heap.free_bytes()>=super::memory::RESERVE+peak+1024&&heap.largest()>=peak;
    let Some(mut job)=(if admitted{JobOwner::try_new(io,control,heap,books,covers)}else{None})else{
        state.mark_settled_slots(all);state.mark_paused_retryable();return R::Declined;};
    if let Some(resume)=state.resume.as_ref(){job.resume=**resume;}
    let outcome=job.service(targets,state.settled_slots()|waiting);
    let resume=job.resume;drop(job); // ALWAYS before the optional resume allocation.
    if outcome.result==R::Cancelled{interrupted(state,resume,outcome.slot,now());}
    else if outcome.slot<4{state.restart();}
    state.mark_settled_slots(outcome.settled);
    if matches!(outcome.result,R::Declined|R::IoError)||(verdict(outcome.result)&&!outcome.no_art){
        state.failures+=1;state.mark_paused_retryable();
    }
    if outcome.result==R::Produced{published();}
    outcome.result
}
