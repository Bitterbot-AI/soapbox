// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

interface Token {
    function transferFrom(address, address, uint256) external returns (bool);
    function transfer(address, uint256) external returns (bool);
    function balanceOf(address) external view returns (uint256);
}

/// @notice Fixed-price compute escrow. The coordinator attests completion; this is not proof of computation.
contract SoapboxEscrow {
    Token public immutable token;
    address public immutable coordinator;
    struct Job { address buyer; address operator; uint256 amount; uint256 expires; uint256 state; bytes32 result; }
    mapping(bytes32 => Job) public jobs;
    uint256 private entered;
    event Funded(bytes32 indexed id, address indexed buyer, address indexed operator, uint256 amount, uint256 expires);
    event Settled(bytes32 indexed id, address indexed operator, uint256 amount, bytes32 result);
    event Refunded(bytes32 indexed id, address indexed buyer, uint256 amount);
    modifier guard() { require(entered == 0, "reentry"); entered = 1; _; entered = 0; }
    constructor(address token_, address coordinator_) {
        require(token_.code.length > 0 && coordinator_ != address(0), "configuration");
        token = Token(token_); coordinator = coordinator_;
    }
    function fund(bytes32 id, address operator, uint256 amount, uint256 expires) external guard {
        require(id != bytes32(0) && jobs[id].state == 0, "duplicate");
        require(operator != address(0) && amount > 0, "quote");
        require(expires >= block.timestamp + 120 && expires <= block.timestamp + 86400, "expiry");
        jobs[id] = Job(msg.sender, operator, amount, expires, 1, bytes32(0));
        uint256 beforeBalance = token.balanceOf(address(this));
        require(token.transferFrom(msg.sender, address(this), amount), "fund transfer");
        require(token.balanceOf(address(this)) == beforeBalance + amount, "token amount");
        emit Funded(id, msg.sender, operator, amount, expires);
    }
    function settle(bytes32 id, bytes32 result) external guard {
        require(msg.sender == coordinator, "coordinator");
        Job storage j = jobs[id];
        require(j.state == 1 && block.timestamp < j.expires && result != bytes32(0), "not settleable");
        j.state = 2; j.result = result;
        require(token.transfer(j.operator, j.amount), "payout transfer");
        emit Settled(id, j.operator, j.amount, result);
    }
    function voidUnfunded(bytes32 id) external {
        require(msg.sender == coordinator && jobs[id].state == 0, "not voidable");
        jobs[id].state = 4;
    }
    function refund(bytes32 id) external guard {
        Job storage j = jobs[id];
        require(j.state == 1, "not funded");
        require(msg.sender == coordinator || block.timestamp >= j.expires, "not refundable");
        j.state = 3;
        require(token.transfer(j.buyer, j.amount), "refund transfer");
        emit Refunded(id, j.buyer, j.amount);
    }
}
