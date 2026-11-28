// Native association APIs, called only by the disposable-host diagnostic.
#import <AppKit/AppKit.h>
#import <CoreServices/CoreServices.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#include <stdio.h>

static void print_json(NSDictionary *value) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:nil];
    fwrite(data.bytes, 1, data.length, stdout);
    puts("");
    fflush(stdout);
}

static NSDictionary *error_json(NSError *error) {
    if (!error) return @{@"ok": @YES};
    NSMutableDictionary *result = [@{@"ok": @NO, @"domain": error.domain,
        @"code": @(error.code), @"description": error.localizedDescription} mutableCopy];
    NSError *underlying = error.userInfo[NSUnderlyingErrorKey];
    if ([underlying isKindOfClass:NSError.class]) result[@"underlying"] = error_json(underlying);
    return result;
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        if (argc < 3) return 2;
        NSString *mode = @(argv[1]);
        NSWorkspace *workspace = NSWorkspace.sharedWorkspace;
        __block BOOL done = NO;
        if ([mode isEqualToString:@"type-query"] && argc == 3) {
            NSString *identifier = @(argv[2]);
            UTType *type = [UTType typeWithIdentifier:identifier];
            NSURL *app = type ? [workspace URLForApplicationToOpenContentType:type] : nil;
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            NSString *all = CFBridgingRelease(LSCopyDefaultRoleHandlerForContentType(
                (__bridge CFStringRef)identifier, kLSRolesAll));
            NSString *viewer = CFBridgingRelease(LSCopyDefaultRoleHandlerForContentType(
                (__bridge CFStringRef)identifier, kLSRolesViewer));
#pragma clang diagnostic pop
            print_json(@{@"all": all ?: @"", @"viewer": viewer ?: @"",
                         @"workspace": app ? ([NSBundle bundleWithURL:app].bundleIdentifier ?: @"") : @""});
            return 0;
        }
        if ([mode isEqualToString:@"resource"] && argc == 3) {
            NSURL *url = [NSURL fileURLWithPath:@(argv[2])];
            id type = nil, volume = nil;
            NSError *error = nil;
            BOOL ok = [url getResourceValue:&type forKey:NSURLContentTypeKey error:&error]
                && [url getResourceValue:&volume forKey:NSURLIsVolumeKey error:&error];
            print_json(ok ? @{@"content_type": ((UTType *)type).identifier ?: @"",
                             @"is_volume": volume ?: @NO} : error_json(error));
            return 0;
        }
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
        if ([mode isEqualToString:@"modern-type"] && argc == 4) {
            UTType *type = [UTType typeWithIdentifier:@(argv[3])];
            if (!type) return 2;
            [workspace setDefaultApplicationAtURL:[NSURL fileURLWithPath:@(argv[2])]
                               toOpenContentType:type
                               completionHandler:^(NSError *error) {
                print_json(error_json(error));
                done = YES;
            }];
        } else if ([mode isEqualToString:@"modern-file-type"] && argc == 4) {
            [workspace setDefaultApplicationAtURL:[NSURL fileURLWithPath:@(argv[2])]
                    toOpenContentTypeOfFileAtURL:[NSURL fileURLWithPath:@(argv[3])]
                               completionHandler:^(NSError *error) {
                print_json(error_json(error));
                done = YES;
            }];
        } else if ([mode isEqualToString:@"scheme"] && argc == 4) {
            [workspace setDefaultApplicationAtURL:[NSURL fileURLWithPath:@(argv[2])]
                            toOpenURLsWithScheme:@(argv[3])
                               completionHandler:^(NSError *error) {
                print_json(error_json(error));
                done = YES;
            }];
        } else if ([mode isEqualToString:@"path"] && argc == 4) {
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
