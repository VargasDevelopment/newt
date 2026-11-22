"""The card's FatFs cache folder reader against a scripted FatFs, with controls.

The Rust image's page walk (rust/salamander/native_io/native/page_entries.cpp) is
the reader's only caller; it adapts each entry to the C boundary.
"""
from pathlib import Path
import re
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'tools'),str(ROOT/'tests')]
from gate_tier import negative_controls
import host_common as host
from host_objects import compiler_environment
from native_output import owned_directory,owned_file
from source_fixture import replace_once

BUILD=ROOT/'.build/page-entry-fatfs'
NATIVE=ROOT/'rust/salamander/native_io/native'
HEADER='page_entry_fatfs.h'

# A scripted FatFs: the R0.15 names and fields the reader uses.
FATFS=r'''#pragma once
#include <cstdint>
#include <cstring>
typedef char TCHAR;
typedef std::uint32_t FSIZE_t;
typedef enum { FR_OK=0, FR_DISK_ERR=1 } FRESULT;
#define AM_DIR 0x10
typedef struct { int index; } FF_DIR;
typedef struct { FSIZE_t fsize; std::uint16_t fdate,ftime; std::uint8_t fattrib; TCHAR altname[13]; TCHAR fname[256]; } FILINFO;
namespace fake {
struct Entry { const char* lfn; const char* sfn; FSIZE_t bytes; std::uint16_t date,time; std::uint8_t attributes; };
inline const Entry* entries=nullptr; inline int count=0,failAt=-1,opens=0,closes=0;
inline bool failClose=false; inline char opened[256]{};
}
inline FRESULT f_opendir(FF_DIR* directory,const TCHAR* path) {
  ++fake::opens; std::strcpy(fake::opened,path); directory->index=0; return FR_OK;
}
// fname holds the whole long name, or the short name when there is none.
inline FRESULT f_readdir(FF_DIR* directory,FILINFO* info) {
  if (directory->index==fake::failAt) return FR_DISK_ERR;
  *info=FILINFO{};
  if (directory->index==fake::count) return FR_OK;
  const auto& entry=fake::entries[directory->index++];
  std::strcpy(info->fname,entry.lfn ? entry.lfn : entry.sfn); std::strcpy(info->altname,entry.sfn);
  info->fsize=entry.bytes; info->fdate=entry.date; info->ftime=entry.time; info->fattrib=entry.attributes;
  return FR_OK;
}
inline FRESULT f_closedir(FF_DIR*) { ++fake::closes; return fake::failClose ? FR_DISK_ERR : FR_OK; }
'''

SOURCE=r'''#include <cassert>
#include <string>
#include <vector>
#include "page_entry_fatfs.h"
using namespace newt::firmware;
using namespace newt::firmware::sd_cache_detail;
struct Seen { std::vector<std::string> names; std::vector<PageEntry> entries; bool ended=false; int stopAt=-1; };
bool visit(void* context,const PageEntry& entry) {
  auto& seen=*static_cast<Seen*>(context);
  if (!entry.name) { seen.ended=true; return true; }
  if (static_cast<int>(seen.names.size())==seen.stopAt) return false;
  seen.names.push_back(entry.name); seen.entries.push_back(entry); return true;
}
bool read(const char* folder,Seen& seen) { return readPageEntries(folder,true,visit,&seen); }
int main() {
  const std::string page(std::string(64,'a')+"-"+std::string(64,'b')+".fibp");
  const std::string temp(page+".00000001.tmp");
  const fake::Entry card[]{
    {page.c_str(),"AAAAAA~1.FIB",4096,0x5a21,0x6000,0x20},
    {temp.c_str(),"AAAAAA~2.TMP",5,0x5a22,0x6001,0x20},
    {nullptr,"AAAAAA~3.FIB",7,0x5a23,0x6002,0x20},  // long name lost or too long
    {nullptr,"?",9,0x5a24,0x6003,0x20},             // neither name readable
    {"nested","NESTED",0,0x5a25,0x6004,0x10},
  };
  fake::entries=card; fake::count=5;
  Seen seen; assert(read("/newt-sd/Newt/Cache/Pages",seen) && seen.ended);
  assert(!std::strcmp(fake::opened,"0:/Newt/Cache/Pages") && fake::closes==1 && "folder_always_closed");
  assert(seen.names.size()==5 && seen.names[0]==page && seen.names[1]==temp && "full_long_name");
  assert(seen.entries[0].bytes==4096 && seen.entries[0].time==(std::int64_t{0x5a21}<<16|0x6000) && seen.entries[0].regular);
  assert(!seen.entries[4].regular && "directory_is_not_regular");
  // A lost long name arrives as the short name, never as a cut long name;
  // the census treats it as unknown (chapter_cache_producer_storage_test).
  assert(seen.names[2]=="AAAAAA~3.FIB" && seen.names[3]=="?");
  // Errors and a declining visit stop the read; the folder is always closed.
  { Seen stop; stop.stopAt=1; fake::closes=0; assert(!read("/newt-sd/Newt/Cache/Pages",stop) && stop.names.size()==1 && !stop.ended && fake::closes==1 && "folder_always_closed"); }
  { Seen failed; fake::failAt=2; fake::closes=0; assert(!read("/newt-sd/Newt/Cache/Pages",failed) && failed.names.size()==2 && !failed.ended && fake::closes==1 && "folder_always_closed"); fake::failAt=-1; }
  { Seen closing; fake::failClose=true; assert(!read("/newt-sd/Newt/Cache/Pages",closing) && closing.ended && "close_failure_reported"); fake::failClose=false; }
  // Only the card's mount is drive 0; anything else is refused unopened.
  fake::opens=0;
  for (auto folder:{"/newt-sdx/Pages","/other/Pages","/newt-sd"}) { Seen refused; assert(!read(folder,refused) && refused.names.empty() && "only_card_mount"); }
  assert(!fake::opens && "only_card_mount");
  std::puts("PASS FatFs folder reader: full long names, short-name fallback, sizes/times/type, every stop closes, card mount only");
}
'''

# Each control breaks one reader rule and must fail its own check.
CONTROLS=(
    ('altname','{end ? nullptr : info.fname,','{end ? nullptr : info.altname,','full_long_name'),
    ('leaked-folder','return f_closedir(&directory)==FR_OK && good;','return good;','folder_always_closed'),
    ('any-mount',"if (std::strncmp(folder,kSdMountPoint,kMount) || folder[kMount]!='/') return false;",
     'if (std::strncmp(folder,kSdMountPoint,kMount)) return false;','only_card_mount'),
)


def run(label,header,diagnostic=None):
    stage=owned_directory(ROOT,BUILD/label,create=True)
    owned_file(ROOT,stage/'ff.h').write_text(FATFS)
    owned_file(ROOT,stage/HEADER).write_text(header)
    source=owned_file(ROOT,stage/'test.cpp');source.write_text(SOURCE)
    binary=owned_file(ROOT,stage/'test')
    # ESP_PLATFORM selects the card's declaration in page_entry_directory.h.
    subprocess.run([host.COMPILER,'--no-default-config','-std=c++17','-Wall','-Wextra','-Werror','-pedantic',
                    *host.SANITIZERS,'-DESP_PLATFORM=1','-I',str(stage),'-I',str(NATIVE),
                    str(source),'-o',str(binary)],check=True,cwd=ROOT,env=compiler_environment(ROOT))
    result=subprocess.run([str(binary)],cwd=ROOT,env=compiler_environment(ROOT),capture_output=True,text=True)
    if diagnostic:
        assert result.returncode and diagnostic in result.stderr,(diagnostic,result.stdout,result.stderr[-2000:])
        print('PASS negative control: '+diagnostic,flush=True)
    else:
        assert result.returncode==0,(result.stdout,result.stderr[-2000:])
        print(result.stdout,end='',flush=True)


def main():
    owned_directory(ROOT,BUILD,create=True)
    production=(NATIVE/HEADER).read_text()
    callers=[path.name for path in sorted(NATIVE.iterdir())
             if path.is_file() and re.search(r'\breadPageEntries\(',path.read_text())]
    assert callers==['page_entries.cpp','page_entry_directory.h',HEADER],callers
    includers=[path.name for path in sorted(NATIVE.iterdir())
               if path.is_file() and f'#include "{HEADER}"' in path.read_text()]
    assert includers==['page_entries.cpp'],includers
    print('PASS the page walk is the one reader caller and the one includer',flush=True)
    run('current',production)
    for label,old,new,diagnostic in CONTROLS if negative_controls(__file__) else ():
        run(label,replace_once(production,old,new),diagnostic)


if __name__=='__main__':
    main()
