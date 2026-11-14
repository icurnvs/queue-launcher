#import <Foundation/Foundation.h>
#import <WebKit/WebKit.h>

void queue_delete_store(const char *id, void (*completed)(int)) {
    NSUUID *uuid = [[NSUUID alloc] initWithUUIDString:[NSString stringWithUTF8String:id]];
    [WKWebsiteDataStore removeDataStoreForIdentifier:uuid completionHandler:^(NSError *error) {
        completed(error ? 1 : 0);
    }];
}
