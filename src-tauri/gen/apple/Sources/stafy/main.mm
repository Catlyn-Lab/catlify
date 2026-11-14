#include "bindings/bindings.h"

extern "C" void stafy_ios_bootstrap(void);

int main(int argc, char * argv[]) {
	// Starts the audio engine, Now Playing/remote commands and CarPlay routing (media/*.swift).
	stafy_ios_bootstrap();
	ffi::start_app();
	return 0;
}
