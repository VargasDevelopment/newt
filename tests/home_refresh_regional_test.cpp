#include "FreeInkDisplay.h"
#include "driver/Uc8253X3Driver.h"
#include "driver/Uc8279Driver.h"
#include "lut/Uc8279X3Luts.h"
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstdlib>
#include <vector>

using freeink::FreeInkDisplay;
using freeink::GrayscaleMode;
using freeink::GrayscaleRegion;
using freeink::BinaryGrayRegion;
using freeink::BinaryGrayRegionPlan;
constexpr BinaryGrayRegion kStageRegions[] = {
    {568,374,8,108}, {568,210,8,108}, {568,46,8,108}};
[[maybe_unused]] constexpr BinaryGrayRegion kSameStageRegions[] = {
    {568,374,8,108}, {568,210,8,108}, {568,46,8,108}};
void require(bool okay, const char* message) {
  if (!okay) { std::fprintf(stderr, "FAIL: %s\n", message); std::exit(1); }
}
unsigned refreshes(const freeink::EpdBus& bus) {
  unsigned count = 0;
  for (const auto& write : bus.writes) count += write.command == 0x12;
  return count;
}
void activation(const freeink::EpdBus& bus, std::array<unsigned, 4> expected) {
  std::vector<std::uint8_t> window;
  unsigned count = 0;
  for (const auto& write : bus.writes) {
    if (write.command == 0x90) window = write.bytes;
    if (write.command == 0x12) {
      require(window.size() == 9 && window[8] == 1, "activation has explicit PTL rectangle");
      const std::array<unsigned, 4> actual = {
        unsigned(window[0]) * 256 + window[1], unsigned(window[2]) * 256 + window[3],
        unsigned(window[4]) * 256 + window[5], unsigned(window[6]) * 256 + window[7]};
      require(actual == expected, "activation native X and gate-space Y are exact");
      ++count;
    }
  }
  require(count == 1, "one combined activation, with no monochrome intermediate");
}
int main() {
#if defined(TEST_UC8279) && TEST_UC8279
  BoardConfig::ACTIVE.displayController = BoardConfig::DisplayController::UC8279;
  BoardConfig::ACTIVE.board = BoardConfig::Board::XteinkX3Uc8279;
  constexpr const char* controller = "UC8279";
#else
  constexpr const char* controller = "UC8253";
#endif
  FreeInkDisplay display(0, 0, 0, 0, 0, 0);
  display.setDisplayX3(); display.begin();
  auto& bus = *freeink::EpdBus::recording;
  constexpr GrayscaleRegion focus{432, 47, 240, 432};
  std::vector<std::uint8_t> lsb(display.getBufferSize()), msb(display.getBufferSize());
  for (unsigned y = 0; y < display.getDisplayHeight(); ++y)
    for (unsigned x = 0; x < display.getDisplayWidthBytes(); ++x) {
      lsb[y * 99 + x] = static_cast<std::uint8_t>(y);
      msb[y * 99 + x] = static_cast<std::uint8_t>(~y);
    }
  auto reject = [&](GrayscaleMode mode, GrayscaleRegion region) {
    const auto before = bus.writes.size();
    require(!display.displayGrayscaleBase(mode, region), "unsupported/unsafe region rejects");
    require(bus.writes.size() == before, "rejection issues no bus write or waveform");
  };
  bus.clear();
  reject(GrayscaleMode::Direct, focus);
  auto full = [&]() {
    bus.clear();
    require(display.displayGrayscaleBase(GrayscaleMode::Direct), "full Direct remains available");
    display.copyGrayscaleBuffers(lsb.data(), msb.data());
    require(refreshes(bus) == 0, "complete selectors stage before activation");
    display.displayGrayBuffer();
    activation(bus, {0, 791, 0, 527});
    bool actualControllerBank = false;
    for (const auto& write : bus.writes) if (write.command == 0x20) {
#if defined(TEST_UC8279) && TEST_UC8279
      const auto* vcom = freeink::kUc8279X3_Xth4[0];
      actualControllerBank = write.bytes == std::vector<std::uint8_t>(vcom, vcom + 49);
#else
      const auto* vcom = freeink::uc8253X3DefaultConfig().directGray->vcom;
      actualControllerBank = write.bytes == std::vector<std::uint8_t>(vcom, vcom + 42);
#endif
    }
    require(actualControllerBank, "facade routes to tested controller's actual Direct bank");
  };
  full();
  constexpr freeink::BinaryGrayIslandRegion island{432,464,240,16};
  auto rejectIsland = [&](freeink::BinaryGrayIslandRegion region, const std::uint8_t* plane) {
    const auto before=bus.writes.size();
    require(!display.displayBinaryGrayIsland(plane,region),"unsafe/unproven binary island rejects");
    require(bus.writes.size()==before,"binary rejection has no commands/plane writes/activation");
  };
  rejectIsland(island,lsb.data()); // Existing full pass has gray selectors inside island.
  [[maybe_unused]] auto rejectBind = [&]() {
    for (auto mode:{GrayscaleMode::Direct,GrayscaleMode::Overlay}) {
      const auto before=bus.writes.size();
      require(!display.displayGrayscaleBaseWithBinaryRegions<kStageRegions,3>(mode),
              "unsupported or invalid binary plan rejects Direct and Overlay");
      require(!display.displayGrayscaleBaseWithBinaryRegions<kStageRegions,3,true>(mode),
              "unsupported neutral-gap plan rejects Direct and Overlay");
      require(bus.writes.size()==before,"plan binding rejection issues no bus write");
    }
  };
#if !defined(FREEINK_EXPERIMENTAL_REGIONAL_GRAY) || !FREEINK_EXPERIMENTAL_REGIONAL_GRAY || !defined(TEST_UC8279) || !TEST_UC8279
  rejectBind();
#endif
#if defined(FREEINK_EXPERIMENTAL_REGIONAL_GRAY) && FREEINK_EXPERIMENTAL_REGIONAL_GRAY && defined(TEST_UC8279) && TEST_UC8279
  static constexpr BinaryGrayRegion unaligned[]={{1,0,8,1}};
  static constexpr BinaryGrayRegion zero[]={{0,0,0,1}};
  static constexpr BinaryGrayRegion outside[]={{784,527,16,1}};
  static constexpr BinaryGrayRegion overlap[]={{0,0,16,2},{8,1,16,2}};
  static constexpr BinaryGrayRegion tooLarge[]={{0,0,792,5}};
  static constexpr BinaryGrayRegion gapDifferentX[]={{568,46,8,1},{576,48,8,1}};
  static constexpr BinaryGrayRegion gapDifferentWidth[]={{568,46,8,1},{568,48,16,1}};
  static constexpr BinaryGrayRegion gapTooTall[]={{568,0,8,1},{568,527,8,1}};
  static constexpr BinaryGrayRegion many[33]{};
  static constexpr BinaryGrayRegionPlan invalidPlans[]={{nullptr,1},{kStageRegions,0},
      {unaligned,1},{zero,1},{outside,1},{overlap,2},{tooLarge,1},{many,33},
      {gapDifferentX,2,true},{gapDifferentWidth,2,true},{gapTooTall,2,true}};
  for (const auto mode:{GrayscaleMode::Direct,GrayscaleMode::Overlay}) {
    for (const auto& invalid:invalidPlans) {
      freeink::Uc8279Driver lowLevel;
      const auto before=bus.writes.size();
      require(!lowLevel.beginGrayscaleWithBinaryRegions(bus,display.getFrameBuffer(),
                  mode,invalid,freeink::RefreshMode::Half,false),
              "low-level driver rejects invalid binary plan geometry");
      require(bus.writes.size()==before,"low-level invalid plan rejects before bus writes");
    }
  }
  auto binaryFull = [&]() {
    for (unsigned y=464;y<480;++y) for (unsigned x=54;x<84;++x)
      lsb[y*99+x]=msb[y*99+x]=static_cast<std::uint8_t>(y+x);
    full();
  };
  binaryFull();
  for (auto region:{freeink::BinaryGrayIslandRegion{431,464,240,16},{432,463,240,16},
                   {432,464,239,16},{432,464,240,15},{432,464,0,16},{65535,65535,65535,65535}})
    rejectIsland(region,lsb.data());
  rejectIsland(island,nullptr);
  auto islandPass = [&](const std::vector<std::uint8_t>& target) {
    bus.clear();
    require(display.displayBinaryGrayIsland(target.data(),island),"completed binary gray baseline accepts fixed island");
    activation(bus,{432,671,48,63});
    std::vector<std::uint8_t> window;
    unsigned newWrites=0,oldWrites=0,planeBytes=0;
    bool activated=false,duBank=false;
    for (const auto& write:bus.writes) {
      if (write.command==0x90) window=write.bytes;
      if (write.command==0x12) activated=true;
      if (write.command==0x20) duBank=write.bytes==std::vector<std::uint8_t>(freeink::kUc8279X3_BwDu[0]+1,freeink::kUc8279X3_BwDu[0]+43);
      if (write.command==0x10||write.command==0x13) {
        require(window==std::vector<std::uint8_t>({1,176,2,159,0,48,0,63,1}),"all plane writes bounded to binary island");
        require(write.bytes.size()==480,"island sends exact 480 bytes per plane");
        require((write.command==0x10)==activated,"OLD sync occurs after DU activation; NEW before it");
        for (unsigned row=0;row<16;++row) for (unsigned x=0;x<30;++x)
          require(write.bytes[row*30+x]==target[(479-row)*99+54+x],"actual selectors emitted bottom-first with correct full-plane stride");
        newWrites+=write.command==0x13;oldWrites+=write.command==0x10;planeBytes+=write.bytes.size();
      }
    }
    require(duBank&&newWrites==1&&oldWrites==1&&planeBytes==960,"existing DU bank with NEW plus OLD sync only 960 bytes");
  };
  // Distinct real targets across repeated passes: every OLD sync is next DU's
  // retained baseline. No full-plane upload or gray resync is allowed here.
  for (unsigned pass=0;pass<12;++pass) {
    for (unsigned y=464;y<480;++y) for (unsigned x=54;x<84;++x)
      msb[y*99+x]=static_cast<std::uint8_t>(pass*17+y-x);
    islandPass(msb);
  }
  constexpr GrayscaleRegion status{8,30,24,134};
  auto statusPass = [&]() {
    bus.clear();
    require(display.displayGrayscaleBase(GrayscaleMode::Direct,status),"battery region accepts completed Direct baseline");
    display.copyGrayscaleBuffers(lsb.data(),msb.data());
    require(refreshes(bus)==0,"battery selectors complete before activation");
    display.displayGrayBuffer(); activation(bus,{8,31,364,497});
    unsigned planes=0;
    for (const auto& write:bus.writes) if (write.command==0x10 || write.command==0x13) {
      const auto& source=write.command==0x10 ? lsb : msb;
      require(write.bytes.size()==source.size(),"battery pass uploads both full selector planes");
      for (unsigned y=0;y<528;++y) for (unsigned x=0;x<99;++x)
        require(write.bytes[y*99+x]==source[(527-y)*99+x],"status uploads preserve every actual selector and bottom-first ordering");
      ++planes;
    }
    require(planes==2,"status uses exactly two complete selector uploads");
  };
  // The latest DU target differs from the initial full-gray cursor. A disjoint
  // pass must compare both actual uploads with the current on-panel checkpoint.
  for (unsigned y=464;y<480;++y) for (unsigned x=54;x<84;++x)
    lsb[y*99+x]=msb[y*99+x];
  for (unsigned repeat=0;repeat<3;++repeat) {
    for (unsigned y=30;y<164;++y) for (unsigned x=1;x<4;++x) {
      lsb[y*99+x]^=0xa5; msb[y*99+x]^=0x5a;
    }
    statusPass(); islandPass(msb);
  }
  // A caller-owned immutable plan generalizes the proof without enlarging its
  // 480-byte checkpoint. Plan identity, not equivalent mutable geometry, binds
  // the complete Direct proof to later subsets.
  auto customFull = [&]() {
    for (const auto& region:kStageRegions) for (unsigned y=region.y;y<region.y+region.height;++y)
      for (unsigned x=region.x/8;x<(region.x+region.width)/8;++x)
        lsb[y*99+x]=msb[y*99+x]=static_cast<std::uint8_t>(y+x);
    bus.clear();
    require(display.displayGrayscaleBaseWithBinaryRegions<kStageRegions,3>(GrayscaleMode::Direct),
            "valid static multi-region plan binds a complete Direct pass");
    display.copyGrayscaleBuffers(lsb.data(),msb.data());display.displayGrayBuffer();
    activation(bus,{0,791,0,527});
  };
  auto rejectStagePlan = [&](std::uint32_t mask) {
    const auto before=bus.writes.size();
    require(!display.displayBinaryGrayRegions<kStageRegions,3>(msb.data(),mask),
            "invalid/unproven binary region set rejects");
    require(bus.writes.size()==before,"whole subset rejects before its first write");
  };
  customFull();
  {
    const auto before=bus.writes.size();
    require(!display.displayBinaryGrayRegions<kSameStageRegions,3>(msb.data(),1),
            "equal static geometry at another address is not the bound immutable object");
    require(bus.writes.size()==before,"different static binding rejects before writes");
  }
  rejectStagePlan(0);
  rejectStagePlan(8);
  bus.clear();
  for (unsigned i:{0u,2u}) {
    const auto& region=kStageRegions[i];
    for (unsigned y=region.y;y<region.y+region.height;++y)
      msb[y*99+region.x/8]=static_cast<std::uint8_t>(i*73+y);
  }
  require(display.displayBinaryGrayRegions<kStageRegions,3>(msb.data(),0b101),
          "one prevalidated subset updates two disjoint binary regions");
  unsigned regionRefresh=0,newWrites=0,oldWrites=0;
  std::vector<std::uint8_t> window;
  bool activated=false;
  for (const auto& write:bus.writes) {
    if (write.command==0x90) window=write.bytes;
    if (write.command==0x12) {++regionRefresh;activated=true;}
    if (write.command!=0x10 && write.command!=0x13) continue;
    const unsigned selected=newWrites+oldWrites<2 ? 0u : 2u;
    const auto& region=kStageRegions[selected];
    const unsigned gateStart=528-(region.y+region.height),gateEnd=527-region.y;
    require(window==std::vector<std::uint8_t>({std::uint8_t(region.x>>8),std::uint8_t(region.x),
                std::uint8_t((region.x+region.width-1)>>8),std::uint8_t(region.x+region.width-1),
                std::uint8_t(gateStart>>8),std::uint8_t(gateStart),
                std::uint8_t(gateEnd>>8),std::uint8_t(gateEnd),1}),
            "each subset plane write and activation uses only its proven rectangle");
    require(write.bytes.size()==108,"Stage proof region sends its exact 108 bytes");
    require((write.command==0x10)==activated,"each region stages NEW, activates, then synchronizes OLD");
    for (unsigned row=0;row<108;++row)
      require(write.bytes[row]==msb[(region.y+107-row)*99+region.x/8],
              "multi-region selectors use the full-plane stride and bottom-first rows");
    newWrites+=write.command==0x13;oldWrites+=write.command==0x10;
    if (write.command==0x10) activated=false;
  }
  require(regionRefresh==2&&newWrites==2&&oldWrites==2,
          "two selected regions perform two bounded DU activations and OLD syncs");
  // Update the full-upload source to the new on-panel subset. A disjoint gray
  // pass may preserve this exact checkpoint; any stale proof byte revokes it.
  for (const auto& region:kStageRegions) for (unsigned y=region.y;y<region.y+region.height;++y)
    for (unsigned x=region.x/8;x<(region.x+region.width)/8;++x) lsb[y*99+x]=msb[y*99+x];
  statusPass();
  bus.clear();require(display.displayBinaryGrayRegions<kStageRegions,3>(msb.data(),0b010),
                      "disjoint Direct-gray activation preserves a matching custom plan");
  lsb[kStageRegions[0].y*99+kStageRegions[0].x/8]^=1;
  statusPass();rejectStagePlan(1);
  binaryFull(); // restore the legacy plan for compatibility/lifecycle cases below
  // Equal LSB/MSB alone does not prove the old on-panel cursor survived. A
  // changed binary cursor outside activation must revoke quiet eligibility.
  lsb[464*99+54]^=1; msb[464*99+54]^=1;
  statusPass(); rejectIsland(island,msb.data());
  binaryFull();
  bus.clear();
  require(display.displayGrayscaleBase(GrayscaleMode::Direct,{432,464,240,16}),"overlapping region accepts gray transaction");
  display.copyGrayscaleBuffers(lsb.data(),msb.data()); display.displayGrayBuffer();
  rejectIsland(island,msb.data());
  // A region never creates proof without a previous full binary baseline.
  msb[464*99+54]^=1;
  full(); statusPass(); rejectIsland(island,msb.data());
  for (unsigned failure=0;failure<4;++failure) {
    binaryFull(); bus.clear();
    require(display.displayGrayscaleBase(GrayscaleMode::Direct,status),"start invalid status upload");
    if (failure==0) display.copyGrayscaleLsbBuffers(lsb.data());
    if (failure==1) display.copyGrayscaleMsbBuffers(msb.data());
    if (failure==2) {
      display.copyGrayscaleBuffers(lsb.data(),msb.data()); display.copyGrayscaleLsbBuffers(lsb.data());
    }
    if (failure==3) display.writeGrayscalePlaneStrip(FreeInkDisplay::GRAY_PLANE_LSB,lsb.data(),1,1);
    display.displayGrayBuffer();
    require(refreshes(bus)==0,"incomplete/out-of-order status never activates");
    rejectIsland(island,msb.data());
  }
  // Full gray cleanup restores selectors/proof; unproven/partial/incomplete
  // transactions and every whole-screen transition invalidate island reuse.
  binaryFull(); display.requestResync(); rejectIsland(island,lsb.data());
  binaryFull(); display.cleanupGrayscaleBuffers(lsb.data()); rejectIsland(island,lsb.data());
  binaryFull(); display.displayBuffer(FreeInkDisplay::FAST_REFRESH); rejectIsland(island,lsb.data());
  binaryFull(); display.setInverted(true); rejectIsland(island,lsb.data());
  display.setInverted(false); rejectIsland(island,lsb.data());
  binaryFull(); display.deepSleep(); rejectIsland(island,lsb.data());
  display.begin(); rejectIsland(island,lsb.data());
  binaryFull(); display.copyGrayscaleLsbBuffers(lsb.data()); rejectIsland(island,lsb.data());
  for (unsigned missing=0;missing<3;++missing) {
    binaryFull();
    require(display.displayGrayscaleBase(GrayscaleMode::Direct),"begin invalid binary proof case");
    if (missing==0) display.copyGrayscaleLsbBuffers(lsb.data());
    if (missing==1) display.copyGrayscaleMsbBuffers(msb.data());
    if (missing==2) {
      display.copyGrayscaleBuffers(lsb.data(),msb.data());
      display.copyGrayscaleLsbBuffers(lsb.data());
    }
    display.displayGrayBuffer(); rejectIsland(island,lsb.data());
  }
  binaryFull();
  bus.clear(); display.displayBuffer(FreeInkDisplay::FAST_REFRESH);
  require(refreshes(bus)>=2,"gray outside island still forces whole monochrome destination rebase");
  rejectIsland(island,lsb.data());
  // Overlay gray: the island's on-panel content is the binary base, captured
  // when the base lands; OLD holds overlay masks, so the DU restores it first.
  auto overlayHome = [&](std::uint8_t seed,bool grayInIsland) {
    auto* fb=display.getFrameBuffer();
    for (unsigned i=0;i<lsb.size();++i) fb[i]=static_cast<std::uint8_t>(i*7+seed);
    std::vector<std::uint8_t> base(fb,fb+lsb.size());
    require(display.displayGrayscaleBase(GrayscaleMode::Overlay),"overlay base");
    std::vector<std::uint8_t> masks(lsb.size(),0x5a);
    for (unsigned y=464;y<480;++y) for (unsigned x=54;x<84;++x) masks[y*99+x]=grayInIsland ? 1 : 0;
    display.copyGrayscaleLsbBuffers(masks.data()); display.copyGrayscaleMsbBuffers(masks.data());
    display.displayGrayBuffer();
    return base;
  };
  auto overlayIsland = [&](const std::vector<std::uint8_t>& old,const std::vector<std::uint8_t>& target) {
    bus.clear();
    require(display.displayBinaryGrayIsland(target.data(),island),"overlay gray with an empty island accepts the island");
    activation(bus,{432,671,48,63});
    unsigned oldBefore=0,newBefore=0,oldAfter=0;
    bool activated=false;
    for (const auto& write:bus.writes) {
      if (write.command==0x12) activated=true;
      if (write.command!=0x10 && write.command!=0x13) continue;
      require(write.bytes.size()==480,"overlay island planes are exactly the island");
      const auto& source=write.command==0x10 && !activated ? old : target;
      for (unsigned row=0;row<16;++row) for (unsigned x=0;x<30;++x)
        require(write.bytes[row*30+x]==source[(479-row)*99+54+x],"overlay island OLD restores on-panel content; NEW is the target");
      oldBefore+=write.command==0x10 && !activated; newBefore+=write.command==0x13 && !activated;
      oldAfter+=write.command==0x10 && activated;
    }
    require(oldBefore==1 && newBefore==1 && oldAfter==1,"OLD and NEW staged before the DU, OLD synchronized after");
  };
  auto base=overlayHome(3,false);
  std::vector<std::uint8_t> first(lsb.size()),second(lsb.size());
  for (unsigned i=0;i<first.size();++i) { first[i]=static_cast<std::uint8_t>(i*5); second[i]=static_cast<std::uint8_t>(i*11+1); }
  overlayIsland(base,first);
  overlayIsland(first,second); // each pass's target is the next pass's on-panel content
  overlayHome(4,true); rejectIsland(island,first.data()); // gray masks inside the island
  overlayHome(5,false); display.displayBuffer(FreeInkDisplay::FAST_REFRESH); rejectIsland(island,first.data());
  overlayHome(6,false); display.cleanupGrayscaleBuffers(lsb.data()); rejectIsland(island,first.data());
  overlayHome(7,false); display.requestResync(); rejectIsland(island,first.data());
  // A due cleanup reaching Overlay Home from black and white (Overlay left as
  // the firmware does, then a DU screen): the base is one differential settle
  // unless requestResync first adds the GC clear.
  for (const bool cleanup : {false,true}) {
    overlayHome(8,false); display.cleanupGrayscaleBuffers(lsb.data());
    display.displayBuffer(FreeInkDisplay::FAST_REFRESH); display.displayBuffer(FreeInkDisplay::FAST_REFRESH);
    if (cleanup) display.requestResync();
    bus.clear(); require(display.displayGrayscaleBase(GrayscaleMode::Overlay),"overlay base");
    require(refreshes(bus)==(cleanup ? 2u : 1u),"only a due cleanup clears the Overlay base with a GC from black and white");
  }
  // The Stage opt-in keeps the three bars as distinct checkpoint regions while
  // issuing one DU over their encompassing vertical strip. The gap rows are
  // controller WW selectors in both planes; physical quietness needs panel evidence.
  auto neutralBase = [&](GrayscaleMode mode,std::uint8_t seed,bool grayInRegion) {
    std::vector<std::uint8_t> base(lsb.size());
    bus.clear();
    if (mode==GrayscaleMode::Direct) {
      for (unsigned i=0;i<lsb.size();++i) {
        lsb[i]=static_cast<std::uint8_t>(i*7+seed);
        msb[i]=static_cast<std::uint8_t>(i*11+seed+1);
      }
      for (const auto& region:kStageRegions)
        for (unsigned y=region.y;y<region.y+region.height;++y)
          msb[y*99+region.x/8]=lsb[y*99+region.x/8];
      base=lsb;
      require(display.displayGrayscaleBaseWithBinaryRegions<kStageRegions,3,true>(mode),
              "neutral-gap Direct plan binds before its complete planes");
      display.copyGrayscaleBuffers(lsb.data(),msb.data());display.displayGrayBuffer();
    } else {
      auto* fb=display.getFrameBuffer();
      for (unsigned i=0;i<base.size();++i) fb[i]=static_cast<std::uint8_t>(i*13+seed);
      std::copy(fb,fb+base.size(),base.begin());
      require(display.displayGrayscaleBaseWithBinaryRegions<kStageRegions,3,true>(mode),
              "neutral-gap Overlay plan binds before synchronous base capture");
      for (const auto& region:kStageRegions)
        for (unsigned y=region.y;y<region.y+region.height;++y) fb[y*99+region.x/8]^=0xff;
      std::vector<std::uint8_t> masks(base.size(),0x5a);
      for (const auto& region:kStageRegions)
        for (unsigned y=region.y;y<region.y+region.height;++y)
          masks[y*99+region.x/8]=grayInRegion ? 1 : 0;
      display.copyGrayscaleLsbBuffers(masks.data());display.copyGrayscaleMsbBuffers(masks.data());
      display.displayGrayBuffer();
    }
    return base;
  };
  auto rejectNeutral = [&](const std::uint8_t* plane,std::uint32_t mask) {
    const auto before=bus.writes.size();
    require(!display.displayBinaryGrayRegions<kStageRegions,3,true>(plane,mask),
            "unproven or invalid neutral-gap update rejects");
    require(bus.writes.size()==before,"neutral-gap rejection precedes every bus write");
  };
  auto neutralPass = [&](std::vector<std::uint8_t>& old,
                         const std::vector<std::uint8_t>& target,std::uint32_t mask) {
    unsigned yStart=528,yEnd=0;
    for (unsigned i=0;i<3;++i) if (mask&(1u<<i)) {
      yStart=std::min<unsigned>(yStart,kStageRegions[i].y);
      yEnd=std::max<unsigned>(yEnd,kStageRegions[i].y+kStageRegions[i].height);
    }
    const bool adjacent=mask==0b011 || mask==0b110;
    if (adjacent) require(yEnd-yStart==272,"adjacent Stage bars span an exact 272-row PTL");
    bus.clear();
    require(display.displayBinaryGrayRegions<kStageRegions,3,true>(target.data(),mask,false),
            "valid neutral-gap subset accepts one bounded DU");
    activation(bus,{568,575,528-yEnd,527-yStart});
    std::vector<std::uint8_t> window;
    unsigned planeWrite=0;
    bool activated=false,duBank=false;
    for (const auto& write:bus.writes) {
      if (write.command==0x90) window=write.bytes;
      if (write.command==0x20)
        duBank=write.bytes==std::vector<std::uint8_t>(&freeink::kUc8279X3_BwDu[0][1],
                                                     &freeink::kUc8279X3_BwDu[0][43]);
      if (write.command==0x12) activated=true;
      if (write.command!=0x10 && write.command!=0x13) continue;
      require(window==std::vector<std::uint8_t>({2,56,2,63,std::uint8_t((528-yEnd)>>8),
                  std::uint8_t(528-yEnd),std::uint8_t((527-yStart)>>8),
                  std::uint8_t(527-yStart),1}),
              "all neutral-gap planes and activation use the selected mask bounds");
      require(write.bytes.size()==yEnd-yStart,"one-byte Stage strip has exact encompassing size");
      require((planeWrite==0 && write.command==0x10 && !activated) ||
              (planeWrite==1 && write.command==0x13 && !activated) ||
              (planeWrite==2 && write.command==0x10 && activated),
              "OLD and NEW precede one DU and target OLD follows it");
      const auto& source=planeWrite==0 ? old : target;
      unsigned wwRows=0;
      for (unsigned row=0;row<yEnd-yStart;++row) {
        const unsigned y=yEnd-1-row;
        bool selected=false;
        for (unsigned i=0;i<3;++i)
          selected=selected || ((mask&(1u<<i)) && y>=kStageRegions[i].y &&
                                y<kStageRegions[i].y+kStageRegions[i].height);
        wwRows+=!selected;
        require(write.bytes[row]==(selected ? source[y*99+71] : 0xff),
                "selected rows use actual checkpoint/frame bytes and every other row is WW");
      }
      if (adjacent) require(wwRows==56,"each adjacent OLD/NEW/OLD strip has one exact 56-row WW gap");
      ++planeWrite;
    }
    require(planeWrite==3 && duBank,"one DU stages both planes and synchronizes OLD once");
    for (unsigned i=0;i<3;++i) if (mask&(1u<<i))
      for (unsigned y=kStageRegions[i].y;y<kStageRegions[i].y+kStageRegions[i].height;++y)
        old[y*99+71]=target[y*99+71];
  };
  auto stageTarget = [&](std::uint8_t seed) {
    std::vector<std::uint8_t> target(lsb.size());
    for (unsigned i=0;i<target.size();++i) target[i]=static_cast<std::uint8_t>(i*17+seed);
    return target;
  };
  for (const auto mode:{GrayscaleMode::Direct,GrayscaleMode::Overlay}) {
    auto checkpoint=neutralBase(mode,static_cast<std::uint8_t>(31+static_cast<unsigned>(mode)),false);
    auto first=stageTarget(7),second=stageTarget(19),third=stageTarget(43),
         fourth=stageTarget(67),fifth=stageTarget(89);
    {
      const auto before=bus.writes.size();
      require(!display.displayBinaryGrayRegions<kStageRegions,3>(first.data(),1),
              "default noncoalesced specialization cannot alias the opted-in plan");
      require(!display.displayBinaryGrayRegions<kSameStageRegions,3,true>(first.data(),1),
              "equal neutral-gap geometry at another address cannot alias plan identity");
      require(bus.writes.size()==before,"typed identity rejects before writes");
    }
    rejectNeutral(first.data(),0);rejectNeutral(first.data(),8);
    neutralPass(checkpoint,first,0b101);
    if (mode==GrayscaleMode::Direct) reject(GrayscaleMode::Direct,status);
    neutralPass(checkpoint,second,0b011);
    neutralPass(checkpoint,third,0b110);
    neutralPass(checkpoint,fourth,0b010);
    neutralPass(checkpoint,fifth,0b101);
    bus.clear();display.displayBuffer(FreeInkDisplay::FAST_REFRESH);
    require(refreshes(bus)>=(mode==GrayscaleMode::Direct ? 2u : 1u),
            "whole paint after neutral gaps takes the mode's existing destination/rebase path");
    rejectNeutral(first.data(),1);
  }
  neutralBase(GrayscaleMode::Overlay,71,true);rejectNeutral(lsb.data(),1);
  neutralBase(GrayscaleMode::Direct,72,false);display.deepSleep();rejectNeutral(lsb.data(),1);display.begin();
  std::puts("PASS: real UC8279 binary gray island proof, 960-byte DU passes, exact PTL, invalidation and whole-panel rebase;"
            " opted-in Stage gaps use one WW-bounded DU with actual Direct/Overlay checkpoints and default plans stay per-region");
#else
  // Even equal selectors cannot enable this path on UC8253 or ordinary builds.
  std::copy(lsb.begin(),lsb.end(),msb.begin());full();rejectIsland(island,lsb.data());
#endif
  full(); // Re-establish Direct before the independent regional-gray contract.

#if defined(FREEINK_EXPERIMENTAL_REGIONAL_GRAY) && FREEINK_EXPERIMENTAL_REGIONAL_GRAY
  require(display.supportsGrayscaleRegion(GrayscaleMode::Direct), "explicit experiment enables region capability");
  require(!display.supportsGrayscaleRegion(GrayscaleMode::Overlay) &&
          !display.supportsGrayscaleRegion(GrayscaleMode::Absolute), "only Direct supports region");
  for (auto region : {GrayscaleRegion{0, 0, 0, 1}, {0, 0, 1, 0}, {792, 0, 1, 1}, {0, 528, 1, 1},
                     {791, 0, 2, 1}, {0, 527, 1, 2}, {65535, 65535, 65535, 65535}}) {
    reject(GrayscaleMode::Direct, region);
  }
  reject(GrayscaleMode::Overlay, focus);
  reject(static_cast<GrayscaleMode>(255), focus);
  bus.clear();
  require(display.displayGrayscaleBase(GrayscaleMode::Direct, focus), "completed full pass permits measured focus region");
  reject(GrayscaleMode::Direct, focus); // May not destroy an in-flight pass.
  display.copyGrayscaleBuffers(lsb.data(), msb.data());
  require(refreshes(bus) == 0, "regional pass keeps complete-plane-before-activation contract");
  unsigned planes = 0;
  for (const auto& write : bus.writes) if (write.command == 0x10 || write.command == 0x13) {
    require(write.bytes.size() == lsb.size(), "both regional pass uploads still cover entire plane");
    const auto& source = write.command == 0x10 ? lsb : msb;
    for (unsigned y = 0; y < 528; ++y) for (unsigned x = 0; x < 99; ++x)
      require(write.bytes[y * 99 + x] == source[(527 - y) * 99 + x], "selectors retain existing bottom-first RAM ordering");
    ++planes;
  }
  require(planes == 2, "exactly two complete RAM planes staged");
  display.displayGrayBuffer(); activation(bus, {432, 671, 49, 480});
  bus.clear();
  require(display.displayGrayscaleBase(GrayscaleMode::Direct, {791, 527, 1, 1}), "edge rectangle valid");
  display.copyGrayscaleBuffers(lsb.data(), msb.data()); display.displayGrayBuffer();
  activation(bus, {784, 791, 0, 0});
  // UC8279's B/W path relies on the persistent PTL window. A regional gray
  // activation must never leave reader RAM writes in the narrow last region.
  bus.clear(); display.displayBuffer(FreeInkDisplay::FAST_REFRESH);
  std::vector<std::uint8_t> restoredWindow;
  unsigned restoredPlanes = 0;
  for (const auto& write : bus.writes) {
    if (write.command == 0x90) restoredWindow = write.bytes;
    if (write.command == 0x10 || write.command == 0x13) {
      require(restoredWindow == std::vector<std::uint8_t>({0, 0, 3, 23, 0, 0, 2, 15, 1}),
              "reader RAM writes restore full 792x528 PTL after regional activation");
      require(write.bytes.size() == lsb.size(), "reader rebase writes full frame");
      ++restoredPlanes;
    }
  }
  require(restoredPlanes >= 2, "reader transition establishes complete monochrome planes");
  reject(GrayscaleMode::Direct, focus);

  // Missing, out-of-order, duplicate, and nonconsecutive planes may never
  // activate or establish a reusable regional baseline.
  for (unsigned failure = 0; failure < 4; ++failure) {
    full(); bus.clear();
    require(display.displayGrayscaleBase(GrayscaleMode::Direct, focus), "start invalid-upload case");
    if (failure == 0) display.copyGrayscaleLsbBuffers(lsb.data());
    if (failure == 1) display.copyGrayscaleMsbBuffers(msb.data());
    if (failure == 2) {
      display.copyGrayscaleBuffers(lsb.data(), msb.data());
      display.copyGrayscaleLsbBuffers(lsb.data());
    }
    if (failure == 3) display.writeGrayscalePlaneStrip(FreeInkDisplay::GRAY_PLANE_LSB, lsb.data(), 1, 1);
    display.displayGrayBuffer();
    require(refreshes(bus) == 0, "invalid upload never activates");
    reject(GrayscaleMode::Direct, focus);
  }
  full(); display.requestResync(); reject(GrayscaleMode::Direct, focus);
  full(); display.cleanupGrayscaleBuffers(lsb.data()); reject(GrayscaleMode::Direct, focus);
  full(); display.displayBuffer(FreeInkDisplay::FAST_REFRESH); reject(GrayscaleMode::Direct, focus);
  full(); display.setInverted(true); reject(GrayscaleMode::Direct, focus);
  display.setInverted(false); reject(GrayscaleMode::Direct, focus);
  full(); display.deepSleep(); reject(GrayscaleMode::Direct, focus);
  display.begin(); reject(GrayscaleMode::Direct, focus);
  full(); display.copyGrayscaleLsbBuffers(lsb.data()); reject(GrayscaleMode::Direct, focus);
  std::printf("PASS: %s experimental regional facade activation bounds, gate mapping, full planes, invalid uploads and baseline recovery\n", controller);
#else
  require(!display.supportsGrayscaleRegion(GrayscaleMode::Direct), "regional experiment is OFF by default");
  reject(GrayscaleMode::Direct, focus);
  full();
  std::printf("PASS: %s default-off regional API rejects without writes; full Direct behavior retained\n", controller);
#endif
  display.releaseBuffers();
  require(!display.framebufferReady(), "host fixture releases its explicit framebuffer ownership");
}
