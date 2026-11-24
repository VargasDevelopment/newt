// SPDX-License-Identifier: MIT
//! Actual automatic Home/shelf cover producer, joined to the single idle owner.
use super::{Runtime,millis,cover_idle::{CoverIdleState,CoverResume,CoverResult,Step},
    cover_resume::CoverResumeOwner};
use crate::{app_image::{Home,Application,Events,Reader},home_startup::CardPort};
use core::ffi::CStr;
use newt_core::{app::Screen,frame::Frame,reader_scheduler::YieldCadence,cover};
use newt_input_policy::native::UiQueue;
#[path="cover_memory.rs"] mod memory;
#[path="cover_io.rs"] mod io;
#[path="cover_member.rs"] mod member;
#[path="cover_xml.rs"] mod xml;
#[path="cover_decode.rs"] mod decode;
#[path="cover_session.rs"] mod session;
#[path="cover_probe.rs"] mod probe;
#[path="cover_job.rs"] mod job;
#[path="cover_idle_step.rs"] mod idle_step;
#[path="cover_observation.rs"] pub(super) mod observation;
#[path="cover_heap.rs"] mod heap;
use io::Control;
use job::Targets;
const BOOKS:&CStr=c"/newt-sd/Newt/Books";
const COVERS:&CStr=c"/newt-sd/Newt/Covers";
struct Poll<'a,'task>{queue:&'a UiQueue<'task>,cadence:YieldCadence}
impl Control for Poll<'_,'_>{
    fn stopped(&self)->bool{
        if self.queue.pending(){return true;}
        #[cfg(feature="dev-usb")]
        if newt_c3_board::dev_usb::available()>0{return true;}
        false
    }
    fn yield_now(&self){self.cadence.service(millis(),||crate::app_power::delay_ms(1),millis);}
}
enum Dispatch{Paint,Quiet,Generated(CoverResult)}
fn dispatch(state:&mut CoverIdleState,screen:Screen,targets:&Targets<'_>,control:&dyn Control,
    work:impl FnOnce(&mut CoverIdleState)->CoverResult)->Dispatch{
    if state.repaint_pending(){
        if state.screen!=screen as u8||state.view!=targets.view{state.consume_repaint();}
        else if control.stopped(){return Dispatch::Quiet;}
        else{
            crate::app_power::delay_ms(10);
            if control.stopped(){return Dispatch::Quiet;}
            state.consume_repaint();return Dispatch::Paint;
        }
    }
    Dispatch::Generated(work(state))
}
fn empty_visit(state:&mut CoverIdleState,screen:Screen,control:&dyn Control)->Dispatch{
    let targets=Targets::default();
    dispatch(state,screen,&targets,control,|state|{
        #[cfg(feature="dev-usb")]
        {let(result,_observation)=observation::collect(millis,|_|prepare_empty(state,screen,&targets,control));result}
        #[cfg(not(feature="dev-usb"))]
        {prepare_empty(state,screen,&targets,control)}
    })
}
fn prepare_empty(state:&mut CoverIdleState,screen:Screen,targets:&Targets<'_>,control:&dyn Control)->CoverResult{
    match idle_step::prepare(state,screen as u8,targets,control,millis){
        Err(result)=>result,Ok(_)=>CoverResult::Busy,
    }
}
#[inline(never)]
fn run_step(state:&mut CoverIdleState,screen:Screen,targets:&Targets<'_>,
    io:&newt_native_io::UiIo,control:&dyn Control,
    #[cfg(feature="dev-usb")] pending:&mut observation::Pending,published:&mut dyn FnMut())->CoverResult{
    #[cfg(feature="dev-usb")]
    {
        let(result,observation)=observation::collect(millis,|heap|
            idle_step::step(state,screen as u8,targets,io,control,heap,millis,BOOKS,COVERS,published));
        pending.record(result,observation);pending.publish(io,control);result
    }
    #[cfg(not(feature="dev-usb"))]
    {idle_step::step(state,screen as u8,targets,io,control,&heap::Heap::new(None),millis,BOOKS,COVERS,published)}
}
fn targets<'a>(names:&'a [[u8;96]],selected:usize,home:bool)->Targets<'a>{
    let mut out=Targets{count:names.len().min(4),selected,view:cover::fnv(2_166_136_261,&[if home{2}else{1}]),..Targets::default()};
    for(slot,name)in names.iter().take(4).enumerate(){
        out.names[slot]=CStr::from_bytes_until_nul(name).ok();
        if let Some(name)=out.names[slot]{out.view=cover::name_key(out.view,name.to_bytes());}
    }
    out
}
impl Runtime{
    pub(super) fn service_covers_image(&mut self,home:&mut Home<'_>,ui:&mut Application<'_>,
        _logical:&mut Frame,events:&mut Events<'_,'_>)->bool{
        let screen=ui.app.screen;
        // Pending DEV rows resume draining only after leaving Reading.
        if screen==Screen::EpubReading{return false;}
        let poll=Poll{queue:events.queue,cadence:YieldCadence::new()};
        #[cfg(feature="dev-usb")]
        {
            if self.cover_observation.ready(&poll){
                let _=home.with_storage_io(|_,io|self.cover_observation.publish(io,&poll));
            }
            if self.cover_observation.draining(){
                crate::app_power::delay_ms(10);return false;
            }
        }
        let mut home_eligible=false;
        if screen==Screen::Home&&ui.home_cover.held().is_none(){
            self.visit_home(&mut|view|home_eligible=view.cover_eligible&&view.identity.is_some_and(|i|i.valid()));
        }
        let shelf=screen==Screen::Library&&!ui.library.no_card&&!ui.library.loading
            &&ui.catalog.as_ref().is_some_and(|c|c.status()==newt_library_current::LibraryStatus::Ok&&c.size()!=0);
        let dispatched=if !shelf&&!home_eligible{empty_visit(&mut self.idle,screen,&poll)}else{
            let result=home.with_storage_io(|home,io|{
                // The optional selected source/receipt stay alive. Return only
                // the actual Pages lease before cover files/allocations begin.
                if let Some(context)=self.session.as_mut(){
                    let context=&mut**context;let mut native=home.card.port(io);
                    let route=context.route.lend(&super::boot::NoCancel,millis);
                    let Ok(mut pages)=newt_native_storage::chapter_cache::ChapterCacheStorage::lend(
                        &mut context.pages,&mut native,&route)else{return Dispatch::Generated(CoverResult::IoError);};
                    pages.access().release_for_state();
                }
                let generation=&mut self.cover_generation;
                let mut published=||Self::note_cover_publication(generation);
                if shelf{
                    let Some(catalog)=ui.catalog.as_mut()else{return empty_visit(&mut self.idle,screen,&poll);};
                    let selected=ui.library.selected.min(catalog.size()-1);
                    let first=newt_library_current_ui::shelf_first(selected);
                    let mut files=newt_native_io::LibraryFiles::new(io);
                    catalog.with_shelf_names(&mut files,first,|names|{
                        let targets=targets(names,selected-first,false);
                        dispatch(&mut self.idle,screen,&targets,&poll,|state|
                            run_step(state,screen,&targets,io,&poll,
                                #[cfg(feature="dev-usb")] &mut self.cover_observation,&mut published))
                    })
                }else{
                    let identity=self.selection.identity();
                    let names=core::slice::from_ref(&identity.filename);
                    let targets=targets(names,0,true);
                    dispatch(&mut self.idle,screen,&targets,&poll,|state|
                        run_step(state,screen,&targets,io,&poll,
                                #[cfg(feature="dev-usb")] &mut self.cover_observation,&mut published))
                }
            });
            match result{Ok(result)=>result,Err(_)=>return false}
        };
        match dispatched{
            Dispatch::Paint=>true,Dispatch::Quiet=>false,Dispatch::Generated(result)=>{
                // Every SDK loan, job allocation and descriptor has ended. The
                // publication callback already advanced the same word before
                // monitor completion. DEV rows may wait for USB; queue repaint here.
                if result==CoverResult::Produced{self.idle.defer_repaint();}
                if result==CoverResult::Cancelled{return false;}
                crate::app_power::delay_ms(10);
                if result!=CoverResult::Produced||poll.stopped(){return false;}
                self.idle.consume_repaint();true
            }
        }
    }
}
