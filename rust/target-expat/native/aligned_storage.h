#pragma once
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <memory>
#include <new>
#include <utility>

namespace newt {
// The pinned C3 allocator promises four-byte alignment. Reserve padding and
// align explicitly before constructing an object or supplying an SDK arena.
inline std::uint8_t* alignedPayload(void* raw, std::size_t prefix, std::size_t alignment) {
  auto* begin=static_cast<std::uint8_t*>(raw)+prefix;
  const auto residue=reinterpret_cast<std::uintptr_t>(begin)%alignment;
  return begin+(residue ? alignment-residue : 0);
}
template<class T> constexpr std::size_t alignedAllocationBytes() {
  static_assert(sizeof(T)<=std::numeric_limits<std::size_t>::max()-sizeof(void*)-(alignof(T)-1));
  return sizeof(T)+sizeof(void*)+alignof(T)-1;
}
template<class T> struct AlignedDelete {
  void operator()(T* value) const {
    if (!value) return;
    std::uint8_t* raw=nullptr;
    std::memcpy(&raw,reinterpret_cast<std::uint8_t*>(value)-sizeof(raw),sizeof(raw));
    value->~T();
    delete[] raw;
  }
};
template<class T> using AlignedOwner=std::unique_ptr<T,AlignedDelete<T>>;
template<class T,class... Args> AlignedOwner<T> makeAlignedOwner(Args&&... args) {
  std::unique_ptr<std::uint8_t[]> allocation{new(std::nothrow) std::uint8_t[alignedAllocationBytes<T>()]};
  if (!allocation) return {};
  auto* raw=allocation.get();
  auto* address=alignedPayload(raw,sizeof(raw),alignof(T));
  std::memcpy(address-sizeof(raw),&raw,sizeof(raw));
  auto* value=new(address) T(std::forward<Args>(args)...);
  allocation.release();
  return AlignedOwner<T>{value};
}
}  // namespace newt
