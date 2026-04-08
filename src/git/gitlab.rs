// TODO
// 这是各种gitee企业版的接口调用方法，供其他模块调用，gitee\gitlab\github三个模块的方法名都一致，只不过内部请求细节以及git token不一致，方便项目页不同仓库使用不同的代码托管平台
// 比如获取merge request列表方法调用的接口参数为：
// url: http://code.devops.piccnet/api/v8/projects/12788/merge_requests?state=all&per_page=10
// headers
// - enterprise: picc
// - OAUTH: enabled
// - Private-Token: xxxxxxxxxxxx
