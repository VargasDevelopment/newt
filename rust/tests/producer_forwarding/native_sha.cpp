// The retained native SHA ABI, copied exactly from oracle.cpp. No policy code.
#include "newt/host_sha256.h"
extern "C" bool producer_passes_hash(const unsigned char* p,unsigned n,unsigned char* digest){return newt::host::sha256(p,n,digest);}
