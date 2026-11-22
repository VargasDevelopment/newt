#pragma once

namespace newt::firmware {
// Caller owns the idle shared bus: no display/card transaction or other SPI
// task may run during this operation. Rebegin on the SAME SPI object because
// Arduino begin() otherwise returns early after display's MISO-less setup.
template <class Spi, class PinsIo, class Board>
bool prepareSharedSdBus(Spi& spi, PinsIo& pins, const Board& board) {
  pins.deselect(board.display.cs);
  pins.deselect(board.sd.cs);
  pins.releaseSdRail();
  pins.settle();
  spi.end();
  return spi.begin(board.display.sclk, board.sd.miso, board.display.mosi, board.sd.cs);
}
}  // namespace newt::firmware
