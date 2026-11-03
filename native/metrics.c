#include <mach/mach.h>
#include <stdint.h>

uint64_t it_resident_bytes(void) {
    mach_task_basic_info_data_t info = {0};
    mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
    kern_return_t result = task_info(mach_task_self(), MACH_TASK_BASIC_INFO,
                                    (task_info_t)&info, &count);
    return result == KERN_SUCCESS ? info.resident_size : 0;
}
