// Native association APIs, called only by the disposable-host diagnostic.
#import <AppKit/AppKit.h>
#import <CoreServices/CoreServices.h>
#include <stdio.h>

static void print_json(NSDictionary *value) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:nil];
    fwrite(data.bytes, 1, data.length, stdout);
    puts("");
    fflush(stdout);
}

static NSDictionary *error_json(NSError *error) {
    return error ? @{@"ok": @NO, @"domain": error.domain, @"code": @(error.code),
                     @"description": error.localizedDescription} : @{@"ok": @YES};
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        if (argc < 3) return 2;
        NSString *mode = @(argv[1]);
        NSWorkspace *workspace = NSWorkspace.sharedWorkspace;
        __block BOOL done = NO;
        if ([mode isEqualToString:@"type"] && argc == 5) {
            LSRolesMask role = [@(argv[3]) isEqualToString:@"viewer"] ? kLSRolesViewer : kLSRolesAll;
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            OSStatus status = LSSetDefaultRoleHandlerForContentType(
                (__bridge CFStringRef)@(argv[2]), role, (__bridge CFStringRef)@(argv[4]));
#pragma clang diagnostic pop
            print_json(@{@"status": @(status), @"ok": @(status == noErr)});
            return 0;
        }
        if ([mode isEqualToString:@"query"] && argc == 3) {
            NSURL *app = [workspace URLForApplicationToOpenURL:[NSURL fileURLWithPath:@(argv[2])]];
            print_json(@{@"bundle_id": app ? ([NSBundle bundleWithURL:app].bundleIdentifier ?: @"") : @"",
                         @"application": app.path ?: @""});
            return 0;
        }
        if ([mode isEqualToString:@"path"] && argc == 4) {
            [workspace setDefaultApplicationAtURL:[NSURL fileURLWithPath:@(argv[2])]
                                 toOpenFileAtURL:[NSURL fileURLWithPath:@(argv[3])]
                               completionHandler:^(NSError *error) {
                print_json(error_json(error));
                done = YES;
            }];
        } else if ([mode isEqualToString:@"open"] && argc == 3) {
            NSWorkspaceOpenConfiguration *configuration = NSWorkspaceOpenConfiguration.configuration;
            configuration.activates = NO;
            [workspace openURL:[NSURL fileURLWithPath:@(argv[2])]
                 configuration:configuration
             completionHandler:^(NSRunningApplication *application, NSError *error) {
                NSMutableDictionary *result = [error_json(error) mutableCopy];
                result[@"recipient"] = application.bundleIdentifier ?: @"";
                result[@"pid"] = @(application ? application.processIdentifier : -1);
                print_json(result);
                done = YES;
            }];
        } else {
            return 2;
        }
        NSDate *limit = [NSDate dateWithTimeIntervalSinceNow:30];
        while (!done && limit.timeIntervalSinceNow > 0)
            [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
        if (!done) {
            print_json(@{@"timeout": @YES});
            return 3;
        }
    }
    return 0;
}
