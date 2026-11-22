// SPDX-License-Identifier: MIT
// Existing SDK operations only. Rust selects planes, refreshes and fallbacks.
#include "board.h"
#include <FreeInkDisplay.h>
namespace newt {
// The panel width and Stage Home's focus marks (rust/stage-home STAGE_FOCUS_*).
inline constexpr int kWidth = 528;
inline constexpr int kStageFocusX = 46, kStageFocusY = 568;
inline constexpr int kStageFocusStep = 164, kStageFocusWidth = 108, kStageFocusHeight = 8;
}  // namespace newt
namespace {
using Display = freeink::FreeInkDisplay;
constexpr freeink::BinaryGrayRegion regions[] = {
    {newt::kStageFocusY, newt::kWidth-newt::kStageFocusX-newt::kStageFocusWidth, newt::kStageFocusHeight, newt::kStageFocusWidth},
    {newt::kStageFocusY, newt::kWidth-newt::kStageFocusX-newt::kStageFocusStep-newt::kStageFocusWidth, newt::kStageFocusHeight, newt::kStageFocusWidth},
    {newt::kStageFocusY, newt::kWidth-newt::kStageFocusX-2*newt::kStageFocusStep-newt::kStageFocusWidth, newt::kStageFocusHeight, newt::kStageFocusWidth}};
}
extern "C" {
int32_t nb_gray_encoding(void* pointer, uint32_t mode, uint32_t* out) {
    if (!pointer || !out) return NB_NULL;
    if (mode > 2) return NB_MODE;
    *out = static_cast<uint32_t>(static_cast<Display*>(pointer)->grayscaleCapabilities(static_cast<freeink::GrayscaleMode>(mode)).encoding);
    return NB_OK;
}
int32_t nb_gray_base(void* pointer, uint32_t mode, uint32_t binary, uint32_t refresh, uint32_t off, uint32_t* accepted) {
    if (!pointer || !accepted) return NB_NULL;
    if (mode > 2 || binary > 1 || refresh > 2 || off > 1) return NB_MODE;
    auto& display = *static_cast<Display*>(pointer);
    const auto selected = static_cast<freeink::GrayscaleMode>(mode);
    const auto fallback = static_cast<Display::RefreshMode>(refresh);
    *accepted = binary ? display.displayGrayscaleBaseWithBinaryRegions<regions,3,true>(selected, fallback, off)
                       : display.displayGrayscaleBase(selected, fallback, off);
    return NB_OK;
}
int32_t nb_gray_copy(void* pointer, uint32_t plane, const uint8_t* bytes, size_t length) {
    if (!pointer) return NB_NULL;
    if (plane > 1) return NB_MODE;
    auto& display = *static_cast<Display*>(pointer);
    if (!bytes) { bytes = display.getFrameBuffer(); length = display.getBufferSize(); }
    if (!bytes || length != 52272) return NB_LENGTH;
    if (plane == 0) display.copyGrayscaleLsbBuffers(bytes);
    else display.copyGrayscaleMsbBuffers(bytes);
    return NB_OK;
}
int32_t nb_gray_display(void* pointer, uint32_t off) {
    if (!pointer) return NB_NULL;
    if (off > 1) return NB_MODE;
    static_cast<Display*>(pointer)->displayGrayBuffer(off != 0);
    return NB_OK;
}
int32_t nb_gray_binary_focus(void* pointer, uint32_t mask, uint32_t off, uint32_t* accepted) {
    if (!pointer || !accepted) return NB_NULL;
    if (off > 1) return NB_MODE;
    auto& display = *static_cast<Display*>(pointer);
    *accepted = display.displayBinaryGrayRegions<regions,3,true>(display.getFrameBuffer(), mask, off != 0);
    return NB_OK;
}
int32_t nb_gray_cleanup_frame(void* pointer) {
    if (!pointer) return NB_NULL;
    auto& display = *static_cast<Display*>(pointer);
    display.cleanupGrayscaleBuffers(display.getFrameBuffer());
    return NB_OK;
}
int32_t nb_gray_resync(void* pointer) {
    if (!pointer) return NB_NULL;
    static_cast<Display*>(pointer)->requestResync();
    return NB_OK;
}
}
